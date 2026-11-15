# Third-party notices

Runtime dependencies, all MIT OR Apache-2.0: sha2 0.11.1, which computes the SHA-256 of the data, and the crates it brings, digest 0.11.3, block-buffer 0.12.1, crypto-common 0.2.2, hybrid-array 0.4.15 and cpufeatures 0.3.1, all by the RustCrypto developers; typenum 1.20.1 (github.com/paholg/typenum); and zeroize 1.9.1 (MIT OR Apache-2.0, by the RustCrypto developers), which wipes buffers that held data.

With the `protect` feature, on by default: secret-protection-core 0.1.0 (MIT OR Apache-2.0), the pages the crate's buffers lie in and the isolation of a process, and the crates it brings: on Unix systems libc 0.2.191 (MIT OR Apache-2.0, by the Rust project developers); on Linux landlock 0.4.7 (MIT OR Apache-2.0, by the Landlock project) with enumflags2 0.7.12 and thiserror 2.0.21 and, at build time only, their macros with proc-macro2 1.0.107, quote 1.0.47, syn 2.0.119 and 3.0.10 and unicode-ident 1.0.27; on Windows windows-sys 0.61.2 with windows-link (MIT OR Apache-2.0, by Microsoft). Its own notices give the authors and licenses of each.

Development dependency, used by the tests only: reed-solomon-erasure 6.0.0 (MIT), an independent Reed–Solomon implementation whose shards the tests expect byte for byte.

The code is the Reed–Solomon code as an evaluation code (Reed and Solomon, 1960); the search for wrong symbols is the Berlekamp–Massey algorithm (Berlekamp, 1968; Massey, 1969). Both are implemented here from their published descriptions. The checksum of the shard file is CRC-32C (Castagnoli, Bräuer and Herrmann, 1993), computed here bit by bit and checked against its published check value.
