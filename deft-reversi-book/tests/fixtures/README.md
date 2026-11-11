# Frozen book inputs

empty-v6.work / empty-v2.book were saved before the shared-value redesign.
search-v7.work / search-v3.book are short v3-pen generation inputs from the preceding implementation. They contain typed search results, profiles and the previous derivation format. Loading these files never searches or needs the model. Keep their bytes fixed; they test old readers during the work8/play4 transition.

- search-v7.work: SHA-256 `22aa348a6d872e243d456e11878f67c857a61f8646b2d21bba30c2a86dd5aa0e`
- search-v3.book: SHA-256 `3257fe030ae4a599ddda6e79ce07c47fee2bd7e4f62b80da5a7fdacdf07224c2`

search-v8.work / search-v4.book were produced from search-v7.work by the frozen pre-maintainability CLI (SHA-256 `c1ab66ed7f1f55d61bcf60a5ea878cbeb96c4c6c245f6ac620b2da5c3fa4b37d`). They verify that the current Rust model can read and resave existing v8/v4 data without changing its bytes.

- search-v8.work: SHA-256 `b3c3f0e08feeee029121200103536b0af0f0f608d3aef6df1a1172237e2d3034`
- search-v4.book: SHA-256 `d40cc2a1f3e6a8cfd902442d7018ff0ebecc49a096ed70920c79ff05a9bb8e0d`
