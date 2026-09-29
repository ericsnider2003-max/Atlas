# "linker `link.exe` not found"

You got further than the last error — Rust installed fine. This is the next
hurdle and it is the last one of its kind.

## What happened

Rust on Windows has two ways to produce a program. The default one borrows
**Microsoft's C++ linker** (`link.exe`), which ships with Visual Studio. You
don't have Visual Studio, so there's nothing to borrow.

## The fix, which is already automatic

Rust also ships a **self-contained toolchain** that brings its own linker and
needs nothing from Microsoft. `install.bat` now detects the missing linker and
switches to it for you. Download the current zip and run it again.

It runs two commands:

```
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

A few hundred megabytes, once. Compare that with the alternative below.

## The other option, if you ever want it

Installing Microsoft's build tools:

```
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet"
```

That's **several gigabytes**, and on a machine with 15.7GB of RAM and a
laptop SSD I'd rather you didn't unless something needs it. Nothing in Atlas
does. The self-contained toolchain produces a program that runs identically.

## Something genuinely useful came out of this

Your error let me compile the Windows-specific parts of Atlas for the first
time. Everything in this project has been built and tested on Linux, and the
file that calls the actual Windows APIs — window enumeration, moving windows,
reading the focused window's title — had **never been through a compiler**. I
have said so in every summary.

Your screenshot let me add that target and check it. **It found one real
error** — a constant that moved between versions of the Windows API bindings —
which is now fixed. The rest compiled clean.

That doesn't mean it *works* yet; compiling and working are different things,
and only running it on your desk will settle that. But an entire category of
"this has never been near a compiler" is now closed.
