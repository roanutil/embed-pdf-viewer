#!/usr/bin/env python3
"""Verify source-pinned Apple native inputs and generated API associations."""
import argparse
import hashlib
import json
from pathlib import Path
import os
import plistlib
import stat
import zipfile
import re
import struct
import subprocess
import sys

BASE = "3c55b7a1c0bf956231392a573e87f28dd6d6667d"
TARGETS = {
    "ios-arm64": ("aarch64-apple-ios", "arm64-apple-ios17.0", "iphoneos", "17.0", 0x100000C, 2),
    "ios-sim-arm64": ("aarch64-apple-ios-sim", "arm64-apple-ios17.0-simulator", "iphonesimulator", "17.0", 0x100000C, 7),
    "darwin-static-arm64": ("aarch64-apple-darwin", "arm64-apple-macosx14.0", "macosx", "14.0", 0x100000C, 1),
    "darwin-static-x64": ("x86_64-apple-darwin", "x86_64-apple-macosx14.0", "macosx", "14.0", 0x1000007, 1),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def digest_matches(path, expected, label):
    require(re.fullmatch(r"[0-9a-f]{64}", expected or ""), f"{label}: missing SHA256")
    require(sha256(path) == expected, f"{label}: hash mismatch: {path}")


def inventory(root):
    root = Path(root)
    return {str(p.relative_to(root)): sha256(p) for p in sorted(root.rglob("*")) if p.is_file()}


def generated_inventory(root):
    files = inventory(root)
    for extension in (".swift", ".h", ".modulemap"):
        require(any(p.endswith(extension) for p in files), f"generated API: missing {extension} in {root}")
    checksums = set()
    for name in files:
        checksums.update(re.findall(r"uniffi_[A-Za-z0-9_]*checksum_[A-Za-z0-9_]+", (Path(root) / name).read_text()))
    require(checksums, f"generated API: no UniFFI checksum exports in {root}")
    return files


def compare_generated(roots):
    require(len(roots) == 4, "generated API: exactly four slice directories required")
    expected = generated_inventory(roots[0])
    for root in roots[1:]:
        require(generated_inventory(root) == expected, f"generated API differs across slices: {root}")
    return expected


def bitcode_facts(data, label):
    dis = os.environ.get("EMBEDPDF_LLVM_DIS")
    analyzer = os.environ.get("EMBEDPDF_LLVM_BCANALYZER")
    require(dis or analyzer, f"unreadable LLVM bitcode: {label}; configure a reviewed compatible LLVM reader")
    require(not (dis and analyzer), "configure only one LLVM bitcode reader")
    command = [dis, "-o", "-"] if dis else [analyzer, "-dump", "-"]
    result = subprocess.run(command, input=data, capture_output=True)
    require(result.returncode == 0, f"LLVM decode failed: {label}: {result.stderr.decode(errors='replace')}")
    text = result.stdout.decode()
    if analyzer:
        records = re.findall(r"<TRIPLE\b[^>]*>", text)
        require(len(records) == 1, f"unreadable or ambiguous LLVM target triple: {label}")
        values = re.findall(r"op[0-9]+=([0-9]+)", records[0])
        require(values and all(int(v) < 128 for v in values), f"unreadable LLVM triple record: {label}")
        text = 'target triple = "' + ''.join(chr(int(v)) for v in values) + '"'
    match = re.search(r'^target triple = "(arm64|aarch64|x86_64)-apple-(macosx|ios)([0-9]+)(?:\.([0-9]+))?(?:\.([0-9]+))?(-simulator)?"$', text, re.MULTILINE)
    require(match, f"unreadable LLVM target/deployment triple: {label}")
    arch, os_name, major, minor, patch, simulator = match.groups()
    cpu = 0x1000007 if arch == "x86_64" else 0x100000C
    platform = 1 if os_name == "macosx" else 7 if simulator else 2
    minimum = (int(major) << 16) | (int(minor or 0) << 8) | int(patch or 0)
    return [(cpu, platform, minimum)]


def object_facts(data, label):
    if data[:4] in (b"BC\xc0\xde", b"\xde\xc0\x17\x0b"):
        return bitcode_facts(data, label)
    require(len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe", f"unreadable native object: {label}; use reviewed compatible object tooling, never skip members")
    _, cpu, _, kind, count, size, _, _ = struct.unpack_from("<8I", data)
    require(kind == 1, f"static archive contains a non-object image: {label}")
    require(32 + size <= len(data), f"truncated Mach-O commands: {label}")
    offset = 32
    versions = []
    for _ in range(count):
        require(offset + 8 <= 32 + size, f"truncated Mach-O command: {label}")
        command, length = struct.unpack_from("<2I", data, offset)
        require(length >= 8 and offset + length <= 32 + size, f"invalid Mach-O command length: {label}")
        if command == 0x32:
            require(length >= 24, f"truncated build version: {label}")
            platform, minimum = struct.unpack_from("<2I", data, offset + 8)
            versions.append((platform, minimum))
        elif command in (0x24, 0x25):
            require(length >= 16, f"truncated minimum OS: {label}")
            minimum = struct.unpack_from("<I", data, offset + 8)[0]
            versions.append((1 if command == 0x24 else 2, minimum))
        offset += length
    require(versions, f"deployment version missing: {label}")
    return [(cpu, platform, minimum) for platform, minimum in versions]


def archive_facts(data, label):
    require(data.startswith(b"!<arch>\n"), f"expected static archive: {label}")
    offset = 8
    facts = []
    while offset < len(data):
        require(offset + 60 <= len(data), f"truncated archive header: {label}")
        header = data[offset:offset + 60]
        require(header[58:60] == b"`\n", f"invalid archive member: {label}")
        length = int(header[48:58].strip())
        name = header[:16].decode("ascii").strip()
        start = offset + 60
        require(start + length <= len(data), f"truncated archive member: {label}/{name}")
        member = data[start:start + length]
        if name.startswith("#1/"):
            name_length = int(name[3:])
            require(name_length <= len(member), f"invalid extended archive name: {label}")
            name = member[:name_length].rstrip(b"\0").decode("utf8")
            member = member[name_length:]
        if name not in ("/", "//", "/SYM64/") and not name.startswith("__.SYMDEF") and not name.endswith((".rmeta", "rust.metadata.bin")):
            facts.extend(object_facts(member, f"{label}/{name}"))
        offset = start + length + length % 2
    require(facts, f"static archive has no inspectable native objects: {label}")
    return facts


def validate_runtime(row, target, expected_runtime):
    rust, swift, sdk, minimum, cpu, platform = TARGETS[target]
    for key, expected in (("rustTarget", rust), ("swiftTarget", swift), ("sdk", sdk), ("minimumOS", minimum)):
        require(row.get(key) == expected, f"{target}: {key} must equal {expected}")
    root = Path(row["runtimeRoot"])
    metadata = json.loads((root / "BUILD-METADATA.json").read_text())
    require(metadata.get("sha") == expected_runtime, f"{target}: runtime metadata source mismatch")
    require(metadata.get("target") == target, f"{target}: runtime metadata target mismatch")
    archive = root / "lib/libembedpdf.a"
    digest_matches(archive, row.get("runtimeArchiveSha256"), f"{target}: runtimeArchiveSha256")
    facts = archive_facts(archive.read_bytes(), str(archive))
    require(all(c == cpu for c, _, _ in facts), f"{target}: architecture mismatch")
    require(all(p == platform for _, p, _ in facts), f"{target}: object platform mismatch")
    floor = int(minimum.split(".")[0]) << 16
    require(max(v for _, _, v in facts) <= floor, f"{target}: deployment exceeds {minimum}")
    digest_matches(root / "args.gn", row.get("gnArgsSha256"), f"{target}: gnArgsSha256")
    args = set((root / "args.gn").read_text().splitlines())
    required = {"pdf_is_complete_lib=true", "embedpdf_thread_local_globals=true", "pdf_enable_brotli=false"}
    required.add(f'mac_deployment_target="{minimum}"' if sdk == "macosx" else f'ios_deployment_target="{minimum}"')
    require(required <= args, f"{target}: required static GN settings missing")
    headers = inventory(root / "include")
    require(headers and headers == row.get("headerHashes"), f"{target}: headerHashes mismatch")


def binary_facts(data, label):
    if data[:4] != b"\xca\xfe\xba\xbe":
        return archive_facts(data, label)
    require(len(data) >= 8, f"truncated universal archive: {label}")
    count = struct.unpack_from(">I", data, 4)[0]
    require(0 < count <= 4 and 8 + count * 20 <= len(data), f"invalid universal architecture table: {label}")
    facts = []
    seen = set()
    for i in range(count):
        cpu, _, offset, size, _ = struct.unpack_from(">5I", data, 8 + i * 20)
        require(cpu not in seen and offset >= 8 + count * 20 and offset + size <= len(data), f"invalid universal architecture slice: {label}")
        seen.add(cpu)
        members = archive_facts(data[offset:offset + size], label)
        require(all(c == cpu for c, _, _ in members), f"universal architecture does not match its archive: {label}")
        facts.extend(members)
    return facts


def framework_inventory(root):
    return {str(p.relative_to(root)): {"symlink": os.readlink(p)} if p.is_symlink() else sha256(p)
            for p in sorted(Path(root).rglob("*")) if p.is_symlink() or p.is_file()}


def validate_xcframework(receipt):
    root = Path(receipt["xcframeworkRoot"])
    files = framework_inventory(root)
    require(files == receipt.get("xcframeworkHashes"), "xcframework hashes do not match tested bytes")
    libraries = plistlib.loads((root / "Info.plist").read_bytes())["AvailableLibraries"]
    expected = {("ios", ""): ({"arm64"}, 2, "17.0"),
                ("ios", "simulator"): ({"arm64"}, 7, "17.0"),
                ("macos", ""): ({"arm64", "x86_64"}, 1, "14.0")}
    require(len(libraries) == 3, "xcframework: exactly three platform entries required")
    seen = set()
    for library in libraries:
        platform = library["SupportedPlatform"]
        variant = library.get("SupportedPlatformVariant", "")
        key = (platform, variant)
        require(key in expected and key not in seen, f"xcframework duplicate or unexpected platform/variant: {key}")
        seen.add(key)
        arches, native_platform, minimum = expected[key]
        require(set(library["SupportedArchitectures"]) == arches, f"xcframework architecture mismatch: {key}")
        framework = root / library["LibraryIdentifier"] / library["LibraryPath"]
        require(framework.is_dir() and framework.suffix == ".framework", f"missing framework: {framework}")
        resolved = framework.resolve()
        require(root.resolve() in resolved.parents, "framework path escapes XCFramework")
        if platform == "macos":
            required_links = {"Versions/Current": "A", "Modules": "Versions/Current/Modules",
                              "Resources": "Versions/Current/Resources", framework.stem: "Versions/Current/" + framework.stem}
            for name, expected_link in required_links.items():
                link = framework / name
                require(link.is_symlink() and os.readlink(link) == expected_link, f"versioned macOS framework link mismatch: {link}")
        binary = framework / framework.stem
        facts = binary_facts(binary.read_bytes(), str(binary))
        cpus = {0x100000C if arch == "arm64" else 0x1000007 for arch in arches}
        require({c for c, _, _ in facts} == cpus, f"framework binary architecture mismatch: {key}")
        require(all(p == native_platform for _, p, _ in facts), f"framework binary platform mismatch: {key}")
        require(max(v for _, _, v in facts) <= int(minimum.split(".")[0]) << 16, f"framework deployment exceeds minimum: {key}")
        plist = framework / ("Resources/Info.plist" if platform == "macos" else "Info.plist")
        minimum_key = "LSMinimumSystemVersion" if platform == "macos" else "MinimumOSVersion"
        require(plistlib.loads(plist.read_bytes()).get(minimum_key) == minimum, f"framework minimum mismatch: {key}")
        modules = framework / "Modules" / (framework.stem + ".swiftmodule")
        for arch in arches:
            suffix = "-simulator" if variant else ""
            interface = modules / f"{arch}-apple-{platform}{suffix}.swiftinterface"
            require(interface.is_file(), f"missing Swift interface: {interface}")
            require("FFI" not in interface.read_text(), f"Swift interface leaks FFI module: {interface}")
    require(seen == set(expected), "missing XCFramework platform")
    archive = Path(receipt["zipPath"])
    digest_matches(archive, receipt.get("zipSha256"), "ZIP checksum")
    with zipfile.ZipFile(archive) as bundle:
        actual = {}
        prefix = root.name + "/"
        for entry in bundle.infolist():
            if entry.is_dir():
                continue
            require(entry.filename.startswith(prefix), "ZIP must contain only the reviewed XCFramework root")
            relative = entry.filename[len(prefix):]
            require(relative not in actual and ".." not in Path(relative).parts and not any(p.startswith("._") for p in Path(relative).parts), "ZIP contains duplicate/unsafe/AppleDouble entry")
            content = bundle.read(entry)
            actual[relative] = {"symlink": content.decode()} if stat.S_ISLNK(entry.external_attr >> 16) else hashlib.sha256(content).hexdigest()
        require(actual == files, "ZIP differs from tested extracted XCFramework")


def validate_receipt(receipt, phase, viewer, runtime):
    require(receipt.get("schemaVersion") == 1, "schemaVersion must be1")
    for key, expected in (("viewerCommit", viewer), ("runtimeCommit", runtime), ("runtimeBase", BASE)):
        require(re.fullmatch(r"[0-9a-f]{40}", expected or "") and receipt.get(key) == expected, f"{key}: expected exact full source commit")
    for key in ("nativeSourceHash", "cargoLockSha256", "toolInputInventorySha256"):
        require(re.fullmatch(r"[0-9a-f]{64}", receipt.get(key, "")), f"{key}: missing source/tool association")
    require(receipt.get("uniffiVersion") == "0.32.0", "uniffiVersion: expected canonical0.32.0")
    for variable, field in (("EMBEDPDF_LLVM_DIS", "llvmDisSha256"), ("EMBEDPDF_LLVM_BCANALYZER", "llvmBcanalyzerSha256")):
        if os.environ.get(variable):
            digest_matches(os.environ[variable], receipt.get(field), field)
    targets = receipt.get("targets", {})
    require(set(targets) == set(TARGETS), "targets: require exactly four Apple slice inputs")
    for target, row in targets.items():
        validate_runtime(row, target, runtime)
    if phase in ("bindings", "xcframework"):
        for target, row in targets.items():
            digest_matches(row["rustArchive"], row.get("rustArchiveSha256"), f"{target}: rustArchiveSha256")
            rust_facts = archive_facts(Path(row["rustArchive"]).read_bytes(), row["rustArchive"])
            require(all(c == TARGETS[target][4] and p == TARGETS[target][5] for c, p, _ in rust_facts), f"{target}: Rust archive architecture/platform mismatch")
            minimum = TARGETS[target][3]
            floor = int(minimum.split(".")[0]) << 16
            require(max(v for _, _, v in rust_facts) <= floor, f"{target}: Rust archive deployment exceeds {minimum}")
            require(generated_inventory(row["generatedRoot"]) == row.get("generatedHashes"), f"{target}: generated hashes mismatch")
        compare_generated([row["generatedRoot"] for row in targets.values()])
    if phase == "xcframework":
        validate_xcframework(receipt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compare-generated", nargs="+")
    parser.add_argument("--phase", choices=("runtime", "bindings", "xcframework"))
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--expected-viewer")
    parser.add_argument("--expected-runtime")
    args = parser.parse_args()
    try:
        if args.compare_generated:
            compare_generated(args.compare_generated)
        else:
            require(args.receipt and args.phase, "--receipt and --phase are required")
            validate_receipt(json.loads(args.receipt.read_text()), args.phase, args.expected_viewer, args.expected_runtime)
    except (ValueError, OSError, KeyError, TypeError, struct.error) as error:
        print(f"native graph: {error}", file=sys.stderr)
        return 1
    print("native graph verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
