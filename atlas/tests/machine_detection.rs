//! The bug this closes: `wants::Machine::default()` was Eric's own laptop's
//! numbers, and nothing built a real one -- so a friend's better hardware
//! got capped at his numbers, and a friend's worse hardware got told it
//! could run things it couldn't. `machine_from` is the fix: build the
//! `Machine` recommendations are filtered against from what `fit` actually
//! measured, not from a fixed laptop's shape.

use atlas::fit::Machine as FitMachine;
use atlas::wants::machine_from;

fn fit_machine(total_ram_mb: u64, free_ram_mb: u64, cpu_cores: u32) -> FitMachine {
    FitMachine {
        total_ram_mb,
        free_ram_mb,
        cpu_cores,
        vram_mb: 0,
        has_npu: false,
        disk_free_mb: 0,
        disk_is_spinning: false,
        reclaimable_mb: 0,
    }
}

/// The exact scenario named as the reason this needed fixing: a friend
/// running Atlas on a much better machine must not be filtered against
/// Eric's laptop's numbers.
#[test]
fn a_friends_better_hardware_is_not_capped_at_erics_laptop() {
    let big = FitMachine { has_npu: false, vram_mb: 24 * 1024, ..fit_machine(64 * 1024, 40 * 1024, 24) };
    let m = machine_from(&big);
    assert_eq!(m.ram_gb, 64.0);
    assert_eq!(m.cpu_cores, 24);
    assert_eq!(m.vram_gb, 24.0);
    assert!(m.ram_gb > machine_default_ram_gb(), "a 64GB machine must not read back as 15.7GB");
}

/// The other half of the same bug: a friend on a *worse* machine must not
/// be told it can do what Eric's laptop can, either.
#[test]
fn a_friends_lesser_hardware_is_not_told_it_can_do_what_erics_laptop_can() {
    let small = fit_machine(8 * 1024, 2 * 1024, 2);
    let m = machine_from(&small);
    assert_eq!(m.ram_gb, 8.0);
    assert_eq!(m.cpu_cores, 2);
    assert!(!m.has_npu);
}

#[test]
fn ram_and_vram_convert_from_megabytes_to_gigabytes_correctly() {
    let m = machine_from(&FitMachine { vram_mb: 8 * 1024, ..fit_machine(16 * 1024, 4 * 1024, 8) });
    assert_eq!(m.ram_gb, 16.0);
    assert_eq!(m.free_ram_gb, 4.0);
    assert_eq!(m.vram_gb, 8.0);
}

/// The specific correction named alongside this fix: reclaimable memory
/// (held by something not in use, which Atlas could offer to close) counts
/// as free. Ignoring it answers "buy more memory" on a day with a browser
/// open, when the honest answer is "close some tabs".
#[test]
fn reclaimable_memory_counts_as_free() {
    let m = machine_from(&FitMachine { reclaimable_mb: 3 * 1024, ..fit_machine(16 * 1024, 2 * 1024, 8) });
    assert_eq!(m.free_ram_gb, 5.0, "2GB genuinely free plus 3GB reclaimable");
}

/// `has_nvidia` is really asking about CUDA, per `Machine`'s own doc --
/// `fit` cannot tell an NVIDIA card from any other dedicated GPU, so
/// guessing yes from VRAM alone would be a wrong claim, not a safe one.
#[test]
fn has_nvidia_is_not_guessed_from_vram_alone() {
    let m = machine_from(&FitMachine { vram_mb: 16 * 1024, ..fit_machine(32 * 1024, 16 * 1024, 16) });
    assert!(!m.has_nvidia, "vram alone doesn't tell you it's an NVIDIA card with CUDA");
}

#[test]
fn has_npu_passes_through_directly_from_fit() {
    let with_npu = machine_from(&FitMachine { has_npu: true, ..fit_machine(16 * 1024, 4 * 1024, 8) });
    assert!(with_npu.has_npu);
    let without = machine_from(&fit_machine(16 * 1024, 4 * 1024, 8));
    assert!(!without.has_npu);
}

#[test]
fn zero_cores_reported_by_the_platform_still_reads_as_at_least_one() {
    // available_parallelism() failing is documented as falling back to 1 in
    // fit::measure -- machine_from should not let a degenerate 0 through
    // either, since a machine with 0 usable cores can't run anything.
    let m = machine_from(&fit_machine(16 * 1024, 4 * 1024, 0));
    assert_eq!(m.cpu_cores, 1);
}

fn machine_default_ram_gb() -> f32 {
    atlas::wants::Machine::default().ram_gb
}
