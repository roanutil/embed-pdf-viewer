# Local native package validation

Run commands from the repository root. Nothing downloads PDFium or publishes
packages. EmbedPDF.Engine remains a provisional package identifier; the local
prerelease version is 0.0.0-local.

1. Build the matching release library with Rust 1.98.0. Set PDF_RUNTIME_LIB_DIR
   to the existing extracted runtime directory and CARGO_TARGET_DIR to the
   job-owned build directory. Example: cargo build -p epdf-capi --release
   --target aarch64-unknown-linux-gnu.
2. Run bash platforms/dotnet/pack.sh --rid linux-arm64 --out /tmp/epdf-local
   --runtime-archive /path/to/libembedpdf-runtime-linux-arm64.tar.gz.
   --native-lib overrides the default Cargo target/release path.
3. Run bash platforms/dotnet/validate.sh --rid linux-arm64 --record
   platforms/dotnet/validation/local-linux-arm64.json --package
   /tmp/epdf-local/EmbedPDF.Engine.0.0.0-local.nupkg --manifest
   /tmp/epdf-local/package-manifest.linux-arm64.json.
4. Inspect the record and native-inspection.json before making any platform
   claim. Packing leaves the RID PACKED_UNVALIDATED; successful manual validation records native VALIDATED or container-emulated explicitly.

Linux commands run inside the approved project container. Set EPDF_IMAGE_ID
to its full immutable SHA256 image identifier, EPDF_HOST_OS to the real host
OS and EPDF_EMULATED=true when the container architecture is emulated.
macOS validation runs on the matching native host. Windows uses
validate.ps1 -Rid win-x64 -Record ... -Package ... -Manifest ... with Python 3,
.NET SDK 10.0.403, Rust 1.98.0 and the Visual Studio dumpbin tool available.
linux-x64: built and tested under emulation on arm64; no native x64 run.
The manifest uses container-emulated for a passing emulated consumer run.
Absent native macOS x64 and Windows x64 hosts remain UNVALIDATED.

The runtime archive must match the existing runtime manifest SHA256 and embedded
source/target identity. --runtime-manifest selects an explicitly reviewed local
pin; --runtime-archive supplies an existing archive. EPDF_RUNTIME_CACHE
selects an existing archive cache. There is no fetch fallback.

--font-manifest is an ordered JSON fonts list. Each item names a relative
path, sha256, relative notice and noticeSha256, plus key, familyName, weight, fallback and optional italic registration fields. Original font bytes and
notices ship in the archive and native NuGet assets. The default inventory is
explicitly empty and cannot establish fallback-font or viewer geometry parity.
The consumer smoke covers ABI, open, pages, text, geometry, region rendering,
pure helpers and missing-library failures from a temporary directory with an
isolated NuGet cache and no source-library environment overrides.

The shipping cdylib uses $ORIGIN or @loader_path; Rust test binaries retain
their source-runtime rpath. ELF inspection rejects absolute dependencies,
nonrelative rpaths and missing libraries and records the highest required GLIBC
symbol version per delivered library. It does not infer support for older
glibc, musl or a different system-library inventory. macOS inspection rejects
build-machine install names/rpaths. PDFium's supplied install names must already
be relocatable; tooling refuses incompatible inputs rather than rewriting
checksum-pinned binaries silently.

Each single-RID local package explicitly lists all five RID statuses. Missing
native macOS x64/Windows hosts, absent artifacts and live viewer parity remain
acceptance limitations. No script claims five-RID support, merges changes,
pushes packages or writes .github/ files.

python3 -m unittest discover -s platforms/dotnet/packaging -v exercises loader
rejection and the checked-in schema vocabulary. The offline schema validator
fails on unsupported schema keywords rather than silently ignoring them.
