# Files, Compression and CSV

Text files are `Str` (`read_file`, `write_file`); binary files are `Bytes`, an immutable byte string: `read_bin(path)` and `write_bin(path, b)` read and write one, `s.to_bytes` and `b.to_str` convert from and to UTF-8, and `bytes([1, 2, 3])`, `b.len`, `b.get(i)`, `b.slice(a, b)`, `b.concat(c)` and `b.to_list` work with the contents. `Bytes` encodes as base64 in JSON and shows as hex, `0x0a1b`.

## Compression

`gzip`, `deflate` (raw DEFLATE, as in zip files) and `zstd` compress `Bytes` (or a `Str`, as its UTF-8) and decompress to a `Res`, with an error message for data that is damaged, truncated or in another format. `compress_level(data, n)` picks the level: 0 to 9 for gzip and DEFLATE (6 by default), 1 to 19 for zstd (3 by default). A gzip file with several members decompresses to their concatenation, as the `gzip` tool does.

```sspur
{{#include ../snippets/handbook/data/files.ssp:compress}}
```

In memory, decompression stops at 1 GiB, which also protects a server from a small upload that expands without end. For larger data, and to keep memory constant whatever the size, `gzip.compress_file(src, dst)` and `decompress_file`, and the same in `zstd`, stream from file to file. The output is written beside `dst` and renamed when it is complete, so a failure never leaves half a file under that name.

The HTTP server's `gzip` field uses the same library, and the HTTP client decodes gzip with the system's (libcurl or WinHTTP).

## CSV

`csv.parse(text)` reads RFC 4180 CSV into rows of fields and `csv.write(rows)` writes it: fields with a comma, a quote or a line break are quoted, a quote inside one is doubled, and lines end with CRLF. The reader also takes LF and CR line endings, skips empty lines and a leading byte order mark, and treats a quote in the middle of an unquoted field as an ordinary character. A quoted field that isn't closed, or has text after its closing quote, is an error that names the row.

`csv.decode[T](text)` reads the first row as a header and each further row as a record `T`, by column name. Fields can be `Str`, `Int`, `F64`, `Bool` (`true`, `false`, `1`, `0`), `Time`, `Dec`, newtypes of them and `Opt` of any, where an empty cell, or a missing column, is `none`. Numbers may have spaces around them. `csv.encode(records)` writes a header and the rows, and decodes back to the same records.

```sspur
{{#include ../snippets/handbook/data/files.ssp:records}}
```

```sspur
{{#include ../snippets/handbook/data/files.ssp:csv}}
```

For a file too large to hold in memory, `csv.fold[T](path, init, f)` reads it a block at a time and folds each record into an accumulator; `csv.read[T](path)` is the whole file as a list. `T` can also be `List[Str]` for raw rows, header included.

```sspur
{{#include ../snippets/handbook/data/files.ssp:stream}}
```

```sspur
{{#include ../snippets/handbook/data/files.ssp:main}}
```

```console
{{#include ../snippets/handbook/data/data.out:files}}
```

Both tiers parse and compress with the same C code (miniz, zstd and SSPUR's CSV reader, built into the cache on first use), so results are byte for byte the same. The [reference](../reference/stdlib/data-libs.md) lists every call.
