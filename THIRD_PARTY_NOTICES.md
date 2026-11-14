# Third-party notices

Runtime dependencies, all MIT OR Apache-2.0: sha2 0.11.1, which computes the SHA-256 of the data, and the crates it brings, digest 0.11.3, block-buffer 0.12.1, crypto-common 0.2.2, hybrid-array 0.4.15 and cpufeatures 0.3.1, all by the RustCrypto developers; and typenum 1.20.1 (github.com/paholg/typenum).

Development dependency, used by the tests only: reed-solomon-erasure 6.0.0 (MIT), an independent Reed–Solomon implementation whose shards the tests expect byte for byte.

The code is the Reed–Solomon code as an evaluation code (Reed and Solomon, 1960); the search for wrong symbols is the Berlekamp–Massey algorithm (Berlekamp, 1968; Massey, 1969). Both are implemented here from their published descriptions. The checksum of the shard file is CRC-32C (Castagnoli, Bräuer and Herrmann, 1993), computed here bit by bit and checked against its published check value.
