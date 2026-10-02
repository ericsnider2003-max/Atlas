//! A model with every input's size written in, for the NPU (item 20).
//!
//! Measured on Eric's laptop (2 Oct 2026): Intel's NPU compiler refused the
//! search model because its inputs' sizes are left open in the file
//! ("upper bounds are not specified"), and neither the provider's
//! `reshape_input` nor ONNX Runtime's dimension overrides reached it. So
//! Atlas writes the sizes into a copy of the model itself -- the same model,
//! the same weights, with `input_ids` saying `[1, 32]` instead of
//! `[batch, sequence]` -- and gives the NPU that copy (`data/cache/npu`).
//!
//! An ONNX file is a protocol buffer. Only the inputs' shapes change, so
//! this walks the few nested messages that lead to them (model → graph →
//! input → type → tensor → shape → dim) and copies every other byte as it
//! is: no ONNX library, nothing about the operators or weights touched.

/// Field numbers, from onnx.proto.
const MODEL_GRAPH: u32 = 7;
const GRAPH_INPUT: u32 = 11;
const VALUE_NAME: u32 = 1;
const VALUE_TYPE: u32 = 2;
const TYPE_TENSOR: u32 = 1;
const TENSOR_SHAPE: u32 = 2;
const SHAPE_DIM: u32 = 1;
const DIM_VALUE: u32 = 1;

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// One field of a message: its number, wire type, and the bytes it covers
/// (tag included), plus the payload range for length-delimited fields.
struct Field {
    number: u32,
    wire: u8,
    whole: std::ops::Range<usize>,
    payload: std::ops::Range<usize>,
}

fn fields(b: &[u8]) -> Option<Vec<Field>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let tag = varint(b, &mut i)?;
        let (number, wire) = ((tag >> 3) as u32, (tag & 7) as u8);
        let payload = match wire {
            0 => {
                let s = i;
                varint(b, &mut i)?;
                s..i
            }
            1 => {
                i += 8;
                (i - 8)..i
            }
            2 => {
                let len = varint(b, &mut i)? as usize;
                let s = i;
                i = i.checked_add(len)?;
                s..i
            }
            5 => {
                i += 4;
                (i - 4)..i
            }
            _ => return None,
        };
        if i > b.len() {
            return None;
        }
        out.push(Field { number, wire, whole: start..i, payload });
    }
    Some(out)
}

/// Rebuild a message, replacing the payload of length-delimited fields
/// numbered `n` with `f(payload)` (`None` from `f` keeps it as it was).
fn rewrite(b: &[u8], n: u32, f: &mut dyn FnMut(&[u8]) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(b.len() + 64);
    for fl in fields(b)? {
        if fl.number == n && fl.wire == 2 {
            if let Some(new) = f(&b[fl.payload.clone()]) {
                put_varint(&mut out, ((n as u64) << 3) | 2);
                put_varint(&mut out, new.len() as u64);
                out.extend_from_slice(&new);
                continue;
            }
        }
        out.extend_from_slice(&b[fl.whole]);
    }
    Some(out)
}

/// The string in a message's field `n`.
fn string_field(b: &[u8], n: u32) -> Option<String> {
    let fl = fields(b)?.into_iter().find(|f| f.number == n && f.wire == 2)?;
    String::from_utf8(b[fl.payload].to_vec()).ok()
}

/// A shape message with exactly these sizes.
fn shape_message(sizes: &[i64]) -> Vec<u8> {
    let mut shape = Vec::new();
    for s in sizes {
        let mut dim = Vec::new();
        put_varint(&mut dim, (DIM_VALUE as u64) << 3);
        put_varint(&mut dim, *s as u64);
        put_varint(&mut shape, ((SHAPE_DIM as u64) << 3) | 2);
        put_varint(&mut shape, dim.len() as u64);
        shape.extend_from_slice(&dim);
    }
    shape
}

/// `model` (the bytes of an .onnx file) with the named inputs' shapes set to
/// the sizes given. Inputs not named are left as they are. `None` when the
/// file can't be read as an ONNX model or a named input isn't in it.
pub fn with_fixed_inputs(model: &[u8], shapes: &[(String, Vec<i64>)]) -> Option<Vec<u8>> {
    let mut fixed = 0usize;
    let out = rewrite(model, MODEL_GRAPH, &mut |graph| {
        rewrite(graph, GRAPH_INPUT, &mut |value| {
            let name = string_field(value, VALUE_NAME)?;
            let (_, sizes) = shapes.iter().find(|(n, _)| *n == name)?;
            let new = rewrite(value, VALUE_TYPE, &mut |ty| {
                rewrite(ty, TYPE_TENSOR, &mut |tensor| {
                    // The shape replaced whole; a tensor with none gets one.
                    let has_shape = fields(tensor)?.iter().any(|f| f.number == TENSOR_SHAPE);
                    let new_shape = shape_message(sizes);
                    if has_shape {
                        rewrite(tensor, TENSOR_SHAPE, &mut |_| Some(new_shape.clone()))
                    } else {
                        let mut t = tensor.to_vec();
                        put_varint(&mut t, ((TENSOR_SHAPE as u64) << 3) | 2);
                        put_varint(&mut t, new_shape.len() as u64);
                        t.extend_from_slice(&new_shape);
                        Some(t)
                    }
                })
            })?;
            fixed += 1;
            Some(new)
        })
    })?;
    if fixed != shapes.len() {
        return None;
    }
    // Read back what was written: every named input carries exactly the
    // sizes asked for, or the copy isn't handed to the NPU compiler.
    let written = input_shapes(&out)?;
    shapes
        .iter()
        .all(|(n, want)| written.iter().any(|(m, got)| m == n && got == want))
        .then_some(out)
}

/// The sizes each input of `model` declares: `(name, dims)`, a dimension
/// left open as `-1`. For checking what was written.
pub fn input_shapes(model: &[u8]) -> Option<Vec<(String, Vec<i64>)>> {
    let graph = fields(model)?.into_iter().find(|f| f.number == MODEL_GRAPH && f.wire == 2)?;
    let g = &model[graph.payload];
    let mut out = Vec::new();
    for input in fields(g)?.into_iter().filter(|f| f.number == GRAPH_INPUT && f.wire == 2) {
        let v = &g[input.payload];
        let name = string_field(v, VALUE_NAME)?;
        let mut dims = Vec::new();
        let ty = fields(v)?.into_iter().find(|f| f.number == VALUE_TYPE)?;
        let ty = &v[ty.payload];
        if let Some(tensor) = fields(ty)?.into_iter().find(|f| f.number == TYPE_TENSOR) {
            let tensor = &ty[tensor.payload];
            if let Some(shape) = fields(tensor)?.into_iter().find(|f| f.number == TENSOR_SHAPE) {
                let shape = &tensor[shape.payload];
                for dim in fields(shape)?.into_iter().filter(|f| f.number == SHAPE_DIM) {
                    let d = &shape[dim.payload];
                    let value = fields(d)?.into_iter().find(|f| f.number == DIM_VALUE && f.wire == 0);
                    dims.push(match value {
                        Some(f) => {
                            let mut i = f.payload.start;
                            varint(d, &mut i)? as i64
                        }
                        None => -1,
                    });
                }
            }
        }
        out.push((name, dims));
    }
    Some(out)
}
