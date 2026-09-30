//! GGUF reader — the model file format, parsed in pure Rust.
//!
//! This is the first piece of replacing Ollama. GGUF holds the weights plus a
//! metadata block describing the architecture, context length, tokenizer and
//! quantization. Ollama reads that block to decide how to load a model and how
//! many layers fit on the GPU. So can we.
//!
//! Format (little-endian throughout):
//!   magic "GGUF" | version u32 | tensor_count u64 | kv_count u64
//!   kv pairs: key string, type u32, value
//!   tensor info: name, n_dims u32, dims[u64], ggml_type u32, offset u64
//!   padding to `general.alignment`, then the tensor data
//!
//! Only the header is read. The weights are never loaded here — inspecting a
//! 40GB model must cost a few kilobytes of I/O, not 40GB of RAM.

use crate::error::{AtlasError, Result};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};

pub const MAGIC: &[u8; 4] = b"GGUF";

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Str(String),
    Array(Vec<Value>),
    U64(u64),
    I64(i64),
    F64(f64),
}

impl Value {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::U8(v) => Some(*v as u64),
            Value::U16(v) => Some(*v as u64),
            Value::U32(v) => Some(*v as u64),
            Value::U64(v) => Some(*v),
            Value::I32(v) if *v >= 0 => Some(*v as u64),
            Value::I64(v) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Value::Array(a) => a.len(),
            _ => 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// ggml tensor types, and how many bits each weight costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantType(pub u32);

impl QuantType {
    pub fn name(&self) -> &'static str {
        match self.0 {
            0 => "F32",
            1 => "F16",
            2 => "Q4_0",
            3 => "Q4_1",
            6 => "Q5_0",
            7 => "Q5_1",
            8 => "Q8_0",
            9 => "Q8_1",
            10 => "Q2_K",
            11 => "Q3_K",
            12 => "Q4_K",
            13 => "Q5_K",
            14 => "Q6_K",
            15 => "Q8_K",
            30 => "BF16",
            _ => "unknown",
        }
    }

    /// Average bits per weight, including the block scales. Used to size a
    /// model before loading it.
    pub fn bits_per_weight(&self) -> f32 {
        match self.0 {
            0 => 32.0,
            1 | 30 => 16.0,
            2 => 4.5,
            3 => 5.0,
            6 => 5.5,
            7 => 6.0,
            8 => 8.5,
            9 => 9.0,
            10 => 2.6,
            11 => 3.4,
            12 => 4.5,
            13 => 5.5,
            14 => 6.6,
            15 => 8.5,
            _ => 8.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TensorInfo {
    pub name: String,
    pub dims: Vec<u64>,
    pub kind: QuantType,
    pub offset: u64,
}

impl TensorInfo {
    pub fn elements(&self) -> u64 {
        self.dims.iter().product()
    }
    pub fn bytes(&self) -> u64 {
        (self.elements() as f64 * self.kind.bits_per_weight() as f64 / 8.0) as u64
    }
}

#[derive(Debug, Clone, Default)]
pub struct Gguf {
    pub version: u32,
    pub metadata: BTreeMap<String, Value>,
    pub tensors: Vec<TensorInfo>,
}

impl Gguf {
    pub fn open(path: &std::path::Path) -> Result<Gguf> {
        let f = std::fs::File::open(path)?;
        Gguf::read(std::io::BufReader::new(f))
    }

    pub fn read<R: Read + Seek>(mut r: R) -> Result<Gguf> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(AtlasError::Platform(
                "not a GGUF file (bad magic) — is this actually a model?".into(),
            ));
        }
        let version = read_u32(&mut r)?;
        if !(1..=3).contains(&version) {
            return Err(AtlasError::Platform(format!("unsupported GGUF version {version}")));
        }
        let tensor_count = read_u64(&mut r)?;
        let kv_count = read_u64(&mut r)?;

        // A corrupt header would otherwise have us allocate wildly.
        if tensor_count > 1_000_000 || kv_count > 100_000 {
            return Err(AtlasError::Platform("GGUF header counts are implausible".into()));
        }

        let mut metadata = BTreeMap::new();
        for _ in 0..kv_count {
            let key = read_string(&mut r)?;
            let ty = read_u32(&mut r)?;
            let val = read_value(&mut r, ty)?;
            metadata.insert(key, val);
        }

        let mut tensors = Vec::with_capacity(tensor_count.min(4096) as usize);
        for _ in 0..tensor_count {
            let name = read_string(&mut r)?;
            let n_dims = read_u32(&mut r)?;
            if n_dims > 8 {
                return Err(AtlasError::Platform("tensor has implausible rank".into()));
            }
            let mut dims = Vec::with_capacity(n_dims as usize);
            for _ in 0..n_dims {
                dims.push(read_u64(&mut r)?);
            }
            let kind = QuantType(read_u32(&mut r)?);
            let offset = read_u64(&mut r)?;
            tensors.push(TensorInfo { name, dims, kind, offset });
        }

        Ok(Gguf { version, metadata, tensors })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// e.g. "llama", "qwen2", "gemma".
    pub fn architecture(&self) -> Option<&str> {
        self.get("general.architecture")?.as_str()
    }

    pub fn name(&self) -> Option<&str> {
        self.get("general.name")?.as_str()
    }

    /// Architecture-prefixed keys, e.g. "llama.context_length".
    fn arch_key(&self, suffix: &str) -> Option<u64> {
        let arch = self.architecture()?;
        self.get(&format!("{arch}.{suffix}"))?.as_u64()
    }

    pub fn context_length(&self) -> Option<u64> {
        self.arch_key("context_length")
    }
    pub fn block_count(&self) -> Option<u64> {
        self.arch_key("block_count")
    }
    fn embedding_length(&self) -> Option<u64> {
        self.arch_key("embedding_length")
    }
    fn head_count(&self) -> Option<u64> {
        self.arch_key("attention.head_count")
    }
    fn head_count_kv(&self) -> Option<u64> {
        self.arch_key("attention.head_count_kv").or_else(|| self.head_count())
    }

    /// The prompt template the model expects, if the file carries one.
    pub fn chat_template(&self) -> Option<&str> {
        self.get("tokenizer.chat_template")?.as_str()
    }

    pub fn total_parameters(&self) -> u64 {
        self.tensors.iter().map(TensorInfo::elements).sum()
    }

    /// Weight bytes on disk.
    pub fn weight_bytes(&self) -> u64 {
        self.tensors.iter().map(TensorInfo::bytes).sum()
    }

    /// The dominant quantization, which is what people mean by "it's a Q4_K_M".
    pub fn dominant_quant(&self) -> QuantType {
        let mut by_bytes: BTreeMap<u32, u64> = BTreeMap::new();
        for t in &self.tensors {
            *by_bytes.entry(t.kind.0).or_default() += t.bytes();
        }
        by_bytes
            .into_iter()
            .max_by_key(|(_, b)| *b)
            .map(|(k, _)| QuantType(k))
            .unwrap_or(QuantType(1))
    }

    /// KV cache bytes for a given context, at f16.
    ///
    /// This is the term people forget. A 7B model at Q4 is ~4GB of weights, but
    /// a 32k context can add several more — which is why a model that "fits"
    /// still runs out of memory partway through a long conversation.
    pub fn kv_cache_bytes(&self, context: u64) -> u64 {
        let (Some(layers), Some(embed), Some(heads)) =
            (self.block_count(), self.embedding_length(), self.head_count())
        else {
            return 0;
        };
        let kv_heads = self.head_count_kv().unwrap_or(heads).max(1);
        let head_dim = self.arch_key("attention.key_length").unwrap_or(embed / heads.max(1));
        // A hybrid model (Qwen3.5: `full_attention_interval` 4) keeps a
        // growing cache only in its attention layers, one in every
        // `interval`; the rest hold a small fixed state (30 Sep 2026: the 9B
        // was sized as if all 33 of its layers cached, a gigabyte too many
        // at 8,192 tokens, which decides whether it may start beside the
        // talking model).
        let layers = match self.arch_key("full_attention_interval") {
            Some(n) if n > 1 => layers.div_ceil(n),
            _ => layers,
        };
        // key + value, 2 bytes each at f16
        2 * 2 * layers * kv_heads * head_dim * context
    }

    /// What it actually costs to run, weights plus cache plus overhead.
    pub fn memory_needed(&self, context: u64) -> u64 {
        let base = self.weight_bytes() + self.kv_cache_bytes(context);
        base + base / 10 // ~10% for compute buffers and fragmentation
    }
}

// --- readers ---

fn read_exact_n<R: Read>(r: &mut R, n: usize) -> Result<Vec<u8>> {
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let b = read_exact_n(r, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u64<R: Read>(r: &mut R) -> Result<u64> {
    Ok(u64::from_le_bytes(eight(r)?))
}

/// Exactly eight bytes, or an error that names the file rather than a panic.
///
/// `read_exact_n(r, 8)` does return eight today, so `try_into().unwrap()` was
/// correct -- and it was one edit to the length argument away from ending the
/// process on a model file someone downloaded. An invariant held by a literal
/// in the line above is not an invariant worth an `unwrap` when the input is
/// a file from the internet.
fn eight<R: Read>(r: &mut R) -> Result<[u8; 8]> {
    let b = read_exact_n(r, 8)?;
    b.try_into().map_err(|_| {
        AtlasError::Platform("the model file ended in the middle of a number".into())
    })
}

fn read_string<R: Read>(r: &mut R) -> Result<String> {
    let len = read_u64(r)?;
    if len > 64 * 1024 * 1024 {
        return Err(AtlasError::Platform("GGUF string implausibly long".into()));
    }
    let b = read_exact_n(r, len as usize)?;
    Ok(String::from_utf8_lossy(&b).to_string())
}

fn read_value<R: Read + Seek>(r: &mut R, ty: u32) -> Result<Value> {
    Ok(match ty {
        0 => Value::U8(read_exact_n(r, 1)?[0]),
        1 => Value::I8(read_exact_n(r, 1)?[0] as i8),
        2 => {
            let b = read_exact_n(r, 2)?;
            Value::U16(u16::from_le_bytes([b[0], b[1]]))
        }
        3 => {
            let b = read_exact_n(r, 2)?;
            Value::I16(i16::from_le_bytes([b[0], b[1]]))
        }
        4 => Value::U32(read_u32(r)?),
        5 => Value::I32(read_u32(r)? as i32),
        6 => Value::F32(f32::from_bits(read_u32(r)?)),
        7 => Value::Bool(read_exact_n(r, 1)?[0] != 0),
        8 => Value::Str(read_string(r)?),
        9 => {
            let elem_ty = read_u32(r)?;
            let count = read_u64(r)?;
            // Token lists run to hundreds of thousands of strings. Keeping the
            // whole vocabulary in memory to answer "how big is this model" is
            // wasteful, so long arrays are counted and skipped.
            if count > 4096 {
                skip_array(r, elem_ty, count)?;
                return Ok(Value::Array(vec![Value::U8(0); count.min(u32::MAX as u64) as usize]));
            }
            let mut items = Vec::with_capacity(count as usize);
            for _ in 0..count {
                items.push(read_value(r, elem_ty)?);
            }
            Value::Array(items)
        }
        10 => Value::U64(read_u64(r)?),
        11 => Value::I64(read_u64(r)? as i64),
        12 => Value::F64(f64::from_le_bytes(eight(r)?)),
        other => return Err(AtlasError::Platform(format!("unknown GGUF value type {other}"))),
    })
}

fn skip_array<R: Read + Seek>(r: &mut R, elem_ty: u32, count: u64) -> Result<()> {
    let fixed = match elem_ty {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4 | 5 | 6 => 4,
        10 | 11 | 12 => 8,
        8 => {
            // Strings are variable length; walk them.
            for _ in 0..count {
                let len = read_u64(r)?;
                r.seek(SeekFrom::Current(len as i64))?;
            }
            return Ok(());
        }
        other => return Err(AtlasError::Platform(format!("cannot skip array of type {other}"))),
    };
    r.seek(SeekFrom::Current((fixed * count) as i64))?;
    Ok(())
}
