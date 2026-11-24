# How reed-solomon-core works

The crate uses two Reed–Solomon constructions. Shard files use polynomial evaluation. Formats such as QR codes prescribe a cyclic generator instead. Both can recover lost symbols and correct wrong ones, but they generally produce different parity bytes. For Rust signatures, return values and examples, see the [API description](API.md).

- [Fields and interpolation](#the-code)
- [Correction and erasures](#correcting-wrong-symbols)
- [Cyclic generator and decoding](#the-cyclic-code)
- [Shard file version 1](#the-shard-file)
- [Recovery and voting across files](#a-set-from-its-files)
- [Memory and work](#memory-and-work)
- [Secrets in memory](#secrets-in-memory)
- [Measurements](#measured-time) and [tests](#self-test-and-tests)

## The code

Each column across the k data shards defines a polynomial of degree below k. Shard i stores its value at field element i. The first k shard indexes hold the original data. Any k distinct points fix the polynomial, so issuing a new shard never changes the earlier ones.

For source points xᵢ and target t, the interpolation weights are

```text
Lᵢ(t) = ∏ⱼ≠ᵢ (t − xⱼ) / (xᵢ − xⱼ)
shard(t) = Σᵢ Lᵢ(t) · shard(xᵢ).
```

Subtraction is XOR. Weights depend only on the indexes. Preparing k source weights takes k(k − 1) multiplications and k field inversions. The same weights serve every column. A target that matches a source can be copied. This construction gives the same systematic coefficients as a Vandermonde matrix normalized on its first k rows.

## Fields

| Field                    | Symbol representation                                   | Arithmetic                                                              |
| ------------------------ | ------------------------------------------------------- | ----------------------------------------------------------------------- |
| GF(256)                  | One byte                                                | Binary polynomial modulo 0x11D.                                         |
| GF(65,536)               | Two bytes, high first                                   | A pair of GF(256) elements, high·x + low, reduced modulo x² + 2x + 128. |
| GF(2,048)                | Two bytes, high first, value below 2,048                | Binary polynomial modulo 0x805.                                         |
| Binary field of degree m | One byte for m = 2…8, two big-endian bytes for m = 9…16 | Binary polynomial modulo the selected primitive polynomial.             |

The point code supports at most 2ᵐ distinct shard indexes. A custom polynomial is given with its leading term and must have degree m and make x primitive. The standard degree-8 and degree-11 polynomials select the named GF(256) and GF(2,048) variants. A degree-16 binary field and the named composite field use different representations. The wire format below accepts only the two designated byte-oriented fields.

## Correcting wrong symbols

Let m denote the distinct shards available at a column, r = m − k, and uᵢ the reciprocal of the product of differences from xᵢ to the other m − 1 points. A valid column has r zero syndromes.

```text
Sⱼ = Σᵢ uᵢ · xᵢʲ · symbolᵢ,    0 ≤ j < r.
```

For a damaged column these are the power sums of its errors. Berlekamp–Massey finds a recurrence of length L. The decoder evaluates its characteristic polynomial at all supplied points and requires L distinct roots and 2L ≤ r. Shard zero is a valid error location. It gives a zero constant term in the degree-L characteristic polynomial. Synthetic division of the product of the root factors recovers the error magnitudes. The decoder verifies them against every syndrome.

### Gaps at known places and a plan of changes

For s marked locations build G(X) = ∏(X − xᵢ), with coefficients g₀ through gₛ. The transformed syndromes Tⱼ = Σᵢ gᵢ Sⱼ₊ᵢ, for 0 ≤ j < r − s, contain only contributions from unknown errors because G vanishes at the marks. The decoder locates these errors and then solves for all e + s magnitudes. Each unknown error costs two spare symbols and each mark costs one. A marked position is reported even when the proposed replacement equals the supplied symbol.

The guaranteed radius is **2e + s ≤ r at each column**. Exactly k unmarked symbols give no redundancy to check. Outside the radius the decoder may return an unresolved column or find a different valid word. A trusted digest or authentication tag is needed to establish which data was intended.

Correction scans first and writes only after all columns have a solution. A preview stores changes in symbol order, then in input-shard order. Its protected or wipe-on-drop buffer has a 32-byte fingerprint followed by 24-byte records. The fingerprint binds the input to the construction and field parameters, shard indexes or positions, lengths, order and bytes. Applying a plan rechecks its fingerprint and each substitution before the first write. Two lists compared with `==` are read whole, values and fingerprints, on the caller's stack: lists with the same changes for other shards differ there, while `get` and `strict::values` show only that the changes agree. [Plan and result types](API.md#plan-and-apply-corrections).

## The cyclic code

A word of length n has d data places followed by p parity places. The first data symbol is the highest coefficient. For β = α^root_step the generator and check symbols are

```text
g(x) = ∏ᵢ₌₀…ₚ₋₁ (x + β^(first_root + i))
parity(x) = data(x) · xᵖ mod g(x).
```

The positive data and parity counts must fit within the order of β, which is (2ᵐ − 1) / gcd(root_step, 2ᵐ − 1) for a primitive α. The implementation uses the binary element 2 and excludes the composite GF(65,536) representation. It supports the primitive-polynomial binary fields, including degree 16. QR uses first root 0 and step 1 in GF(256). MHFE-REPAIR-1 uses first root 1 and step 1 in GF(2,048).

Place i has locator X = β^(n − 1 − i). The decoder evaluates the word at each generator root, treats specified erasures as zero and forms their locator Γ(z) = ∏(1 + Xz). Berlekamp–Massey finds the remaining error locator Λ. The combined locator Ψ = ΛΓ must have the expected number of distinct roots among the n places. For Ω = SΨ mod zᵖ, Forney’s formula gives each magnitude as X^(1 − first_root) Ω(X⁻¹) / Ψ′(X⁻¹). All resulting syndromes must vanish. The same bound 2e + s ≤ p applies to each word. The code reads a symbol only once it knows that its place is not among the erasures; a compiler may still load such a symbol, but its value takes part in no result.

## The shard file

Version 1 stores one shard per file. Each integer is big-endian.

```text
header | block 0, CRC | block 1, CRC | … | identical header | trailer
```

### The header

| Offset        | Bytes   | Meaning                                                                   |
| ------------- | ------- | ------------------------------------------------------------------------- |
| 0             | 4       | ASCII SHRD (53 48 52 44 hex).                                             |
| 4             | 1       | Version 1.                                                                |
| 5             | 1       | Field ID 1 for GF(256), 2 for the composite GF(65,536). ID 3 is reserved. |
| 6             | 1       | Bit 0 signals SHA-256. All other bits are zero.                           |
| 7             | 1       | Identifier length I, from 1 to 32.                                        |
| 8             | I       | Application-chosen set identifier.                                        |
| 8 + I         | 4       | Threshold k, from 1 to the field order.                                   |
| 12 + I        | 4       | Shard index, strictly below the field order.                              |
| 16 + I        | 8       | Original data length in bytes.                                            |
| 24 + I        | 4       | Positive block size in bytes, aligned to a whole symbol.                  |
| 28 + I        | 0 or 32 | Optional SHA-256 of the original data.                                    |
| Final 4 bytes | 4       | CRC-32C over all preceding header bytes.                                  |

The header length H is 33–96 bytes. Files in one set share every set fact, including lengths and digest. Only the shard index and the header CRC derived from it can differ. `SetFacts::new` takes a digest as given, for facts known otherwise, such as those of a header that was read; `Header::file` and `Header::file_into` write the facts they are given beside any shard, so files written one by one can state a digest the data does not have.

### Blocks, the second header and the trailer

Divide the data length by k, round up, then round up to a whole symbol to get shard length S. Split the data into consecutive S-byte chunks and zero-pad unused bytes. There are B = ⌈S / block_size⌉ blocks per shard. Empty data has zero blocks. A final short block carries its own checksum.

The four-byte block CRC covers the header’s four CRC bytes followed by the four-byte block number (starting at zero) and the payload of that block. After the second header, the 12-byte trailer consists of ASCII SHRT, H as a four-byte integer, and a four-byte CRC of those first eight bytes. File length is 2H + S + 4B + 12.

CRC-32C uses polynomial 0x1EDC6F41, reflected input and output, and 0xFFFFFFFF for both initialization and final XOR. The check value for ASCII “123456789” is 0xE3069283. At most 2³² blocks fit because the last block number is 0xFFFFFFFF. File length must fit u64. With a digest, data length must be below 2⁶¹ bytes. Size arithmetic uses wider intermediates before narrowing.

### Reading

The reader inspects the start and the physical end without scanning for signatures inside the payload. An end header qualifies as sound only if its trailer validates and its own sizes describe that exact file length. A version-1 header with an unknown flag or field needs a valid checksum to count as unsupported. A future version can have a different layout, so the magic and version byte alone identify it.

| Start \ end     | sound                               | unsupported     | another version       | damaged     | absent       |
| --------------- | ----------------------------------- | --------------- | --------------------- | ----------- | ------------ |
| sound           | the same: read; different: conflict | read, see below | read                  | read        | read         |
| unsupported     | conflict                            | unsupported     | unsupported           | unsupported | unsupported  |
| another version | read                                | unsupported     | version, of the start | version     | version      |
| damaged         | read                                | unsupported     | version, of the end   | damaged     | damaged      |
| absent          | read                                | unsupported     | version, of the end   | damaged     | unrecognized |

A sound start also determines the expected position of the duplicate header. A different header classified as sound or unsupported there causes a conflict. A version-1 duplicate with a valid CRC and invalid parameters, such as a zero threshold, is damaged. An ordinary appended tail is reported as extra bytes. A different sound header and trailer at the new physical end that exactly describe the extended file can also cause a conflict. A sound end copy can recover a damaged start when the file has the expected length.

Each complete block with a matching CRC is intact. A complete block with a wrong CRC is damaged. An incomplete payload or checksum is missing. These rules also apply to truncated files. CRC detects accidental damage. An adversary able to replace the contents can recompute both CRC and the optional SHA-256 in the header.

### A set from its files

The reader groups files by the full set facts. Two sets with the same identifier and different facts remain separate. For each block, each shard index contributes at most one holder. Disagreeing checksum-valid copies of the same index put that shard in dispute for that block. Reading caches checksum results. A mapped input must remain unchanged while the pile or a derived set is in use.

The lowest k available holder indexes provide the interpolation basis for that block. Exactly k holders leave nothing to compare. With more, each extra holder must agree. A mismatch identifies which shards differ from the basis, which may itself contain a wrong shard. Different blocks may use different holders. Reconstruction verifies zero padding and any declared digest over exactly the original data length. Repairing one or more shard files preserves the declared digest without performing that whole-data check.

### Voting

Of m undisputed holders, let w disagree with a candidate word and let u count disputed shard indexes for which no copy supports it. The vote accepts a candidate when **2w + u ≤ m − k**. Duplicate copies never add witnesses. A competing word agrees with fewer than k of the agreeing holders and cannot collect enough other witnesses to beat this candidate. If the evidence does not meet the rule, the block can remain undecided. A vote changes no input and identifies files by their original positions.

For speed, the decoder remembers the locations of disagreeing holders within each block. It checks each nonzero syndrome sequence against their recurrence before doing another search. A search can enlarge the list. The block is abandoned when the list exceeds ⌊(m − k)/2⌋ or a search fails.

## Memory and work

Point correction processes stripes of syndromes of at most 4 MiB. Field elements, weights, factors, counters and scratch add a term that depends on shard count. The reported decoder bound counts requested working storage. Process RSS also includes page rounding, guard mappings, allocator overhead and caller-owned data. Cyclic correction reuses its syndrome and search buffers between passes and prepares erasure metadata before each pass. Its caller-buffer search uses 10(p + 1) elements of type u16. Verification evaluates the contribution of every located error or erasure to every syndrome.

File writers and rebuilders process one block at a time. The set writer may also allocate padding buffers for the tails of data shards. For b = min(block_size, shard_length), it requests one b-byte result block and (k − whole)·b bytes for tails, where whole counts complete data shards. A pile stores a per-file bitmap of intact blocks. Conflicting copies can also require a per-shard dispute bitmap and the positions of the files involved.

### Work limits for file sets

These steps are accounting units. Preparing interpolation from the k lowest holders costs k² on the first block and whenever the basis changes. For a block of s symbols, synthesizing a missing target or comparing an extra holder costs k(s + 64). Copying an available block adds zero. Batch repairs can share the basis and comparisons.

For a vote write r = m − k, t = ⌊r/2⌋ and d for the number of disputed shard indexes. Blocks with too few holders establish no candidate.

- With r ≥ 1, syndromes cost rm(s + 64). Preparing weights costs m² when the holders differ from the last block that used weights. Blocks with m ≤ k leave that cache intact.
- With t ≥ 1, a disagreeing column costs r(t + 1). Searching it costs a further (4m + 64)(r + 1). At most min(s, t + 1) columns need searching. With t = 0 the first disagreement ends the block without a column charge.
- Comparing disputed indexes costs k² + dk(s + 64) when d > 0, m ≥ k and the holders establish a candidate.

The estimate assumes every column disagrees and all possible searches and dispute checks are needed. The actual count can be lower. In the five-file test with k = 3 and a two-symbol block, one wrong file costs 945 steps against a bound of 1,197. CRC scans, copying and SHA-256 take time outside this count, and so does memory. A vote keeps one search while the number of holders stays the same, and when the holders change it computes only their weights again. When their number changes it makes a new search with two buffers of its own, which with `protect` are pages that system calls map, lock and unmap: a pile whose number of holders changes at every block pays those calls at every block. Every `Buffer` with `protect` is a mapping of its own, so a call that makes tens of thousands of them, as a set of that many shards does, can reach the system's limit on mappings and then answers `Memory`.

## Secrets in memory

The scaling kernel processes eight bytes at a time. Shard bytes select neither branches nor table entries in scaling or CRC-32C. Single field multiplication and inversion avoid tables and branches on their operands, except that an inverse tests its operand for zero, which has none; in the fields of `Field::binary` this rests on `black_box` keeping a mask from becoming a jump, which only the compiled code shows and the timing probe checks before a release. The comparison of buffers and of plans reads every byte; the comparison of the copies of a block stops where they first differ, which shows where copies differ, not what they hold. Decoding still branches on error-dependent results, including discrepancies, roots and the number of wrong symbols. Its running time can reveal information about errors.

Owned data and derived scratch are wiped on normal drop. With `protect` the crate requests guarded pages and reports which OS protections were obtained. The system may give less than it is asked for: pages beyond its lock limit stay unlocked, and the protection crate's own constructors (`WorkArea::new`, `Secret::with_capacity`) succeed all the same, while `WorkArea::locked` and `Secret::locked` refuse. `Buffer::is_locked`, `Buffer::is_out_of_dumps` and `Buffer::is_out_of_children` say what a buffer got, and `Changes::is_locked` what a list got. Windows keeps at most 512 blocks of a process out of its error reports, so a buffer beyond them stays in, and says so. Crash dumps and other programs that read the memory of the process are shut out for the whole process only by `protect::Isolation::enter`, which reports what each system confirmed: that is the program's call, as a library does not change its host's process by itself. With `alloc` alone it uses allocator memory. Ordinary caller-provided input, mappings and output buffers remain the caller’s responsibility. Indexes, counts and public metadata may live outside protected pages.

### The strict mode

Strict data calls check the protection of supplied storage before processing it. Each new internal buffer is checked on allocation before any secret is written into that buffer. A secret-bearing operation runs on a dedicated, wiped 512 KiB stack. Required pages are locked and excluded from child processes. Linux and Android also require per-mapping dump exclusion. On Apple systems a program that needs secrets out of dumps confirms `protect::Isolation`'s report before it hands them over. A shortage of locked memory can cause a refusal even when ordinary RAM is plentiful.

Windows and WebAssembly lack the protected stack required by these calls. Constructors using only public parameters can still succeed. No thread of the process may fork during a strict call. A child must reopen files rather than reuse the parent’s protected views. The guarantee ends at the caller’s storage, callbacks, disk writes, hibernation and the host. [Strict Rust interface](API.md#handle-protected-data).

## Measured time

`cargo run --release --example measure` measures the calls on the machine it runs on, each case in a process of its own. On an Intel Core i7-1260P under Linux 7.0, in one thread, on 2026-11-15:

| Call               | Shape                                                             |   Data | Reading the files | The call | MiB/s | ns per step | Held before | Added by the call |
| ------------------ | ----------------------------------------------------------------- | -----: | ----------------: | -------: | ----: | ----------: | ----------: | ----------------: |
| `Code::encode`     | GF(2^8), k = 10, 4 shards made                                    | 80 MiB |                   |   0.35 s |   229 |             |      82 MiB |            32 MiB |
| `Code::encode`     | GF(2^16), k = 100, 20 shards made                                 | 25 MiB |                   |   1.09 s |    23 |             |      27 MiB |             5 MiB |
| `Code::correct`    | GF(2^8), k = 10 + 4, two wrong symbols at one offset in a hundred | 80 MiB |                   |   1.26 s |    64 |             |     114 MiB |             4 MiB |
| `Code::correct`    | GF(2^16), k = 100 + 20, the same                                  | 25 MiB |                   |   2.65 s |     9 |             |      32 MiB |             4 MiB |
| `FileSet::rebuild` | GF(2^8), k = 10 + 4, 4 data files lost                            | 64 MiB |            0.29 s |   0.30 s |   210 |         1.1 |      92 MiB |            64 MiB |
| `FileSet::rebuild` | GF(2^16), k = 200 + 50, 50 data files lost                        | 32 MiB |            0.14 s |   3.66 s |     9 |         4.4 |      43 MiB |            31 MiB |
| `FileSet::repairs` | GF(2^8), the 4 lost files                                         | 64 MiB |            0.29 s |   0.40 s |   161 |         1.5 |      92 MiB |            25 MiB |
| `FileSet::repairs` | GF(2^16), the 50 lost files                                       | 32 MiB |            0.15 s |   3.70 s |     9 |         4.4 |      43 MiB |             7 MiB |

Reading the files is `Pile::new`, which checks every block's checksum. The sets state no digest, so the time of a call is that of the work its steps count. "Held before" is the resident memory of the process with its inputs, "added" what the call added at its peak: the shards or files it made, its data, and for a correction its syndromes, at most 4 MiB.

The same encoding and rebuilds built for WebAssembly and run once in Node 26, whose engine is Chrome's, with the input made beforehand and the result let go afterwards, took 0.13 seconds to encode 20 MiB in GF(2^8) (156 MiB/s), 0.09 seconds to rebuild 16 MiB in GF(2^8) at 1.4 nanoseconds a step, and 1.03 seconds to rebuild 8 MiB in GF(2^16) at 4.9 nanoseconds a step. The harness for that run is not in the repository, which holds no Node.js script.

## Self-test and tests

The built-in self-test runs 17 checks with an allocator and 11 without one. Its independent fixtures include QR parity, CRC-32C, SHA-256 and reference interpolation. A changed-shard check tests an internal property. Agreement between the allocating, caller-buffer and strict interfaces tests consistency across paths.

The integration suite checks the published MHFE repair words and compares GF(256) and composite GF(65,536) shards with reed-solomon-erasure 6.0.0. Small evaluation codes are rebuilt from every k-subset in all three named fields. The nearest-word decoder oracle uses GF(256). For GF(2,048) the long-division test samples products, while a separate exhaustive test checks the field laws and the scaling kernel. Fixed file vectors cover header bytes, trailer precedence, truncation and block checksums. See the [test suite](../tests/suite), [field tests](../src/field) and [coverage report](COVERAGE.md).

## References

- ISO/IEC 18004:2015, QR code bar code symbology specification, annex I: the worked example of a version 1-M symbol.
- MHFE-REPAIR-1, the optional profile of repair words of the MHFE specification: its generators and published vectors.

- I. S. Reed, G. Solomon, "Polynomial codes over certain finite fields", Journal of the Society for Industrial and Applied Mathematics 8(2), 1960.
- E. R. Berlekamp, Algebraic Coding Theory, McGraw-Hill, 1968.
- J. L. Massey, "Shift-register synthesis and BCH decoding", IEEE Transactions on Information Theory 15(1), 1969.
- G. Castagnoli, S. Bräuer, M. Herrmann, "Optimization of cyclic redundancy-check codes with 24 and 32 parity bits", IEEE Transactions on Communications 41(6), 1993: the polynomial of CRC-32C. RFC 3720, section 12.1 and appendix B.4, has its conventions as used here.
- M. O. Rabin, "Efficient dispersal of information for security, load balancing, and fault tolerance", Journal of the ACM 36(2), 1989.
- J. S. Plank, "A tutorial on Reed–Solomon coding for fault-tolerance in RAID-like systems", Software: Practice and Experience 27(9), 1997; J. S. Plank, Y. Ding, "Note: correction to the 1997 tutorial on Reed–Solomon coding", Software: Practice and Experience 35(2), 2005.
- [Backblaze JavaReedSolomon](https://github.com/Backblaze/JavaReedSolomon), [klauspost/reedsolomon](https://github.com/klauspost/reedsolomon), [reed-solomon-erasure](https://crates.io/crates/reed-solomon-erasure): implementations of the same code over GF(2^8).
