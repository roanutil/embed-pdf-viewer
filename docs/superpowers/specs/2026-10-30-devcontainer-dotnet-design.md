# .NET SDK in the development container

The native binding needs a .NET 10 compiler in the existing Linux development
container. Install it through the checked-in Dockerfile, preserving amd64 and
arm64 support and the unprivileged `node` development user. Running containers
are not modified. Native macOS and Windows validation still needs those hosts.

## Approach

Use Microsoft's SDK 10.0.403 tarballs with their published architecture-specific
SHA512 checksums, verified before extraction. This follows the existing image's
downloaded-toolchain pattern. An apt feed adds repository/version-resolution
state; a downloaded installer adds another executable dependency. Neither is
needed for two pinned archives.

Install under `/opt/dotnet`, expose `DOTNET_ROOT` and `PATH`, and provide the
`dotnet` command to login shells. Existing Debian packages already supply
libclang and .NET's ICU, OpenSSL, Kerberos and zlib dependencies. Verify them
in the resulting image rather than adding duplicate package installations.

## Acceptance

Build fresh images from the committed definition for both supported Linux
architectures. As `node`, verify SDK 10.0.403 in login and non-login shells,
compile and run a scratch net10.0 program, and call a compiled C function through
`LibraryImport`. Verify libclang can be loaded. Record image IDs and platforms;
clearly distinguish native and emulated validation.

No binding project, global SDK policy, product code, package release or change
to existing toolchain pins belongs in this slice. Document the SDK pin and the
rebuild/update procedure in `.devcontainer/README.md`.

Archive URLs and SHA512 source:
https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json
