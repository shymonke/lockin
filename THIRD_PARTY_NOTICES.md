# Third-party notices

LockIn's own code is MIT-licensed (`LICENSE`). `lockin.exe` also contains, or ships next to, the
third-party work listed below. If you distribute a build, include this file, `LICENSE`, `models/` license
files, `assets/fonts/PHOSPHOR-LICENSE.txt` and `vendor/WebView2Loader.LICENSE.txt`.

## Bundled files

| Component | Where | License | Notice |
|---|---|---|---|
| YuNet face detector | `models/yunet.onnx`, compiled in | MIT, Copyright (c) 2020 Shiqi Yu | `models/LICENSE-YUNET-MIT`, `models/NOTICE.md` |
| MediaPipe Face Landmarker (mesh + blendshapes), converted to ONNX | `models/face_landmarks.onnx`, `models/face_blendshapes.onnx`, compiled in | Apache-2.0, Copyright Google LLC | `models/LICENSE-APACHE-2.0`, `models/NOTICE.md` |
| RTMDet-tiny (COCO), cut before NMS | `models/rtmdet.onnx`, compiled in | Apache-2.0, Copyright OpenMMLab | `models/LICENSE-APACHE-2.0`, `models/NOTICE.md` |
| Phosphor Icons (subset) | `assets/fonts/phosphor-lockin.ttf`, compiled in | MIT, Copyright (c) 2020-2021 Phosphor Icons | `assets/fonts/PHOSPHOR-LICENSE.txt` |
| Microsoft WebView2 loader, SDK 1.0.3800.47 (unmodified) | `vendor/WebView2Loader.dll`, shipped next to the exe | BSD-style, Copyright (C) Microsoft Corporation | `vendor/WebView2Loader.LICENSE.txt` |
| egui default fonts: Ubuntu-Light, Hack, Noto Emoji, emoji-icon-font, egui-icons (via `epaint_default_fonts`) | compiled in | Ubuntu Font Licence 1.0, OFL-1.1, MIT | license texts in the [`epaint_default_fonts`](https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts/fonts) crate |

The cat illustrations and the built-in alert sounds are drawn and synthesised in code (`src/memes.rs`,
`src/audio.rs`); no image or audio files are bundled.

## Rust crates

Compiled into `lockin.exe` from crates.io (normal dependencies for `x86_64-pc-windows-gnu`, from
`Cargo.lock`). Each crate's license text is in its source package.

The Symphonia crates (MP3/WAV decoding for custom alert sounds) are MPL-2.0 and are used unmodified;
their source is at https://github.com/pdeljanov/Symphonia. `self_cell` is used under Apache-2.0.

Regenerate this list with `cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-gnu`
after changing dependencies.

| Crate | Version | License | Source |
|---|---|---|---|
| accesskit | 0.25.2 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 |
| ahash | 0.8.12 | MIT OR Apache-2.0 | https://github.com/tkaitchuck/ahash |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | https://github.com/dtolnay/anyhow |
| anymap3 | 1.1.0 | BlueOak-1.0.0 OR MIT OR Apache-2.0 | https://github.com/reivilibre/anymap3 |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | https://github.com/bluss/arrayvec |
| bit-set | 0.11.1 | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec |
| bit-vec | 0.10.1 | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec |
| bitflags | 1.3.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| borsh | 1.8.1 | MIT OR Apache-2.0 | https://github.com/near/borsh-rs |
| borsh-derive | 1.8.1 | Apache-2.0 | https://github.com/near/borsh-rs |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| byteorder | 1.5.0 | Unlicense OR MIT | https://github.com/BurntSushi/byteorder |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | https://github.com/image-rs/byteorder-lite |
| bytes | 1.12.1 | MIT | https://github.com/tokio-rs/bytes |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/stream-ciphers |
| color | 0.3.3 | Apache-2.0 OR MIT | https://github.com/linebender/color |
| color_quant | 1.1.0 | MIT | https://github.com/image-rs/color_quant.git |
| cookie | 0.18.2 | MIT OR Apache-2.0 | https://github.com/SergioBenitez/cookie-rs |
| core_detect | 1.0.0 | MIT OR Apache-2.0 | https://github.com/thomcc/core_detect |
| cpal | 0.18.2 | Apache-2.0 | https://github.com/RustAudio/cpal |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/cursor-icon |
| dasp_sample | 0.11.0 | MIT OR Apache-2.0 | https://github.com/rustaudio/sample.git |
| deranged | 0.5.9 | MIT OR Apache-2.0 | https://github.com/jhpratt/deranged |
| derive-new | 0.7.0 | MIT | https://github.com/nrc/derive-new |
| document-features | 0.2.12 | MIT OR Apache-2.0 | https://github.com/slint-ui/document-features |
| downcast-rs | 2.0.2 | MIT OR Apache-2.0 | https://github.com/marcianx/downcast-rs |
| dpi | 0.1.2 | Apache-2.0 AND MIT | https://github.com/rust-windowing/winit |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | https://gitlab.com/kornelski/dunce |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 | https://github.com/dtolnay/dyn-clone |
| dyn-hash | 1.0.0 | MIT OR Apache-2.0 | https://github.com/dtolnay/dyn-hash |
| ecolor | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| eframe | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/eframe |
| egui | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| egui-winit | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/egui-winit |
| egui_glow | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/egui_glow |
| either | 1.19.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/either |
| emath | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/emath |
| encoding_rs | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause | https://github.com/hsivonen/encoding_rs |
| epaint | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/epaint |
| epaint_default_fonts | 0.37.0 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| erased-serde | 0.4.11 | MIT OR Apache-2.0 | https://github.com/dtolnay/erased-serde |
| euclid | 0.22.14 | MIT OR Apache-2.0 | https://github.com/servo/euclid |
| extended | 0.1.0 | MIT | https://github.com/depp/extended-rs |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | https://github.com/image-rs/fdeflate |
| fearless_simd | 0.4.1 | Apache-2.0 OR MIT | https://github.com/linebender/fearless_simd |
| filetime | 0.2.29 | MIT OR Apache-2.0 | https://github.com/alexcrichton/filetime |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| float-ord | 0.3.2 | MIT  OR  Apache-2.0 | https://github.com/notriddle/rust-float-ord |
| foldhash | 0.2.0 | Zlib | https://github.com/orlp/foldhash |
| font-types | 0.12.7 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| gif | 0.14.2 | MIT OR Apache-2.0 | https://github.com/image-rs/image-gif |
| glifo | 0.3.0 | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| glow | 0.17.0 | MIT OR Apache-2.0 OR Zlib | https://github.com/grovesNL/glow |
| glutin | 0.32.3 | Apache-2.0 | https://github.com/rust-windowing/glutin |
| glutin-winit | 0.5.0 | MIT | https://github.com/rust-windowing/glutin |
| glutin_egl_sys | 0.7.1 | Apache-2.0 | https://github.com/rust-windowing/glutin |
| glutin_wgl_sys | 0.6.1 | Apache-2.0 | https://github.com/rust-windowing/glutin |
| guillotiere | 0.7.0 | MIT OR Apache-2.0 | https://github.com/nical/guillotiere |
| half | 2.7.1 | MIT OR Apache-2.0 | https://github.com/VoidStarKat/half-rs |
| harfrust | 0.12.0 | MIT | https://github.com/harfbuzz/harfrust |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| http | 1.5.0 | MIT OR Apache-2.0 | https://github.com/hyperium/http |
| image | 0.25.10 | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| inventory | 0.3.25 | MIT OR Apache-2.0 | https://github.com/dtolnay/inventory |
| itertools | 0.14.0 | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools |
| itertools | 0.15.0 | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools |
| itoa | 1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| kurbo | 0.13.1 | Apache-2.0 OR MIT | https://github.com/linebender/kurbo |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | https://github.com/rust-lang-nursery/lazy-static.rs |
| libloading | 0.8.9 | ISC | https://github.com/nagisa/rust_libloading/ |
| libm | 0.2.16 | MIT | https://github.com/rust-lang/compiler-builtins |
| linebender_resource_handle | 0.1.1 | Apache-2.0 OR MIT | https://github.com/linebender/raw_resource_handle |
| litrs | 1.0.1 | MIT OR Apache-2.0 | https://github.com/LukasKalbertodt/litrs |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| log | 0.4.35 | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| maplit | 1.0.2 | MIT OR Apache-2.0 | https://github.com/bluss/maplit |
| matrixmultiply | 0.3.11 | MIT OR Apache-2.0 | https://github.com/bluss/matrixmultiply/ |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| memmap2 | 0.9.12 | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs |
| metrics | 0.24.6 | MIT | https://github.com/metrics-rs/metrics |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/moxcms.git |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT | https://github.com/hsivonen/multiversion_no_op |
| ndarray | 0.17.2 | MIT OR Apache-2.0 | https://github.com/rust-ndarray/ndarray |
| nohash-hasher | 0.2.0 | Apache-2.0 OR MIT | https://github.com/paritytech/nohash-hasher |
| nom | 8.0.0 | MIT | https://github.com/rust-bakery/nom |
| nom-language | 0.1.0 | MIT | https://github.com/rust-bakery/nom |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 | https://github.com/rust-num/num-bigint |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | https://github.com/rust-num/num-complex |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | https://github.com/jhpratt/num-conv |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | https://github.com/rust-num/num-integer |
| num-rational | 0.4.2 | MIT OR Apache-2.0 | https://github.com/rust-num/num-rational |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| pastey | 0.2.3 | MIT OR Apache-2.0 | https://github.com/as1100k/pastey |
| peniko | 0.6.1 | Apache-2.0 OR MIT | https://github.com/linebender/peniko |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite |
| png | 0.18.1 | MIT OR Apache-2.0 | https://github.com/image-rs/image-png |
| polycool | 0.4.0 | MIT OR Apache-2.0 | https://github.com/linebender/kurbo |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 | https://github.com/jhpratt/powerfmt |
| primal-check | 0.3.4 | MIT OR Apache-2.0 | https://github.com/huonw/primal |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | https://github.com/bkchr/proc-macro-crate |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| profiling | 1.0.18 | MIT OR Apache-2.0 | https://github.com/aclysma/profiling |
| prost | 0.14.4 | Apache-2.0 | https://github.com/tokio-rs/prost |
| prost-derive | 0.14.4 | Apache-2.0 | https://github.com/tokio-rs/prost |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/pxfm |
| quote | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| rand | 0.10.3 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | https://github.com/rust-random/rand_core |
| rand_distr | 0.6.0 | MIT OR Apache-2.0 | https://github.com/rust-random/rand_distr |
| rapidhash | 4.5.1 | MIT OR Apache-2.0 | https://github.com/hoxxep/rapidhash |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle |
| rawpointer | 0.2.1 | MIT OR Apache-2.0 | https://github.com/bluss/rawpointer/ |
| rayon | 1.12.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| read-fonts | 0.41.0 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| rodio | 0.23.0 | MIT OR Apache-2.0 | https://github.com/RustAudio/rodio |
| rustfft | 6.4.1 | MIT OR Apache-2.0 | https://github.com/ejmahler/RustFFT |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion |
| same-file | 1.0.6 | Unlicense OR MIT | https://github.com/BurntSushi/same-file |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| self_cell | 1.3.0 | Apache-2.0 OR GPL-2.0-only | https://github.com/Voultapher/self_cell |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| simd-adler32 | 0.3.10 | MIT | https://github.com/mcountryman/simd-adler32 |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | https://github.com/rusticstuff/simdutf8 |
| skrifa | 0.44.0 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| smol_str | 0.2.2 | MIT OR Apache-2.0 | https://github.com/rust-analyzer/smol_str |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs |
| strength_reduce | 0.2.4 | MIT OR Apache-2.0 | http://github.com/ejmahler/strength_reduce |
| string-interner | 0.20.0 | MIT OR Apache-2.0 | https://github.com/robbepop/string-interner |
| symphonia | 0.5.5 | MPL-2.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-bundle-mp3 | 0.5.5 | MPL-2.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-core | 0.5.5 | MPL-2.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-format-riff | 0.5.5 | MPL-2.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-metadata | 0.5.5 | MPL-2.0 | https://github.com/pdeljanov/Symphonia |
| syn | 2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| syn | 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| tar | 0.4.46 | MIT OR Apache-2.0 | https://github.com/composefs/tar-rs |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| time | 0.3.55 | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| time-core | 0.1.9 | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| time-macros | 0.2.32 | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_edit | 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| tracing | 0.1.44 | MIT | https://github.com/tokio-rs/tracing |
| tracing-core | 0.1.36 | MIT | https://github.com/tokio-rs/tracing |
| tract-core | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-data | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-extra | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-hir | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-linalg | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-nnef | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-onnx | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-onnx-opl | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| tract-transformers | 0.23.9 | MIT OR Apache-2.0 | https://github.com/sonos/tract |
| transpose | 0.2.3 | MIT OR Apache-2.0 | https://github.com/ejmahler/transpose |
| typeid | 1.0.3 | MIT OR Apache-2.0 | https://github.com/dtolnay/typeid |
| unicode-general-category | 1.1.0 | Apache-2.0 | https://github.com/yeslogic/unicode-general-category |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| unicode-properties | 0.1.4 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-properties |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation |
| uuid | 1.29.0 | Apache-2.0 OR MIT | https://github.com/uuid-rs/uuid |
| vello_common | 0.2.0 | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| vello_cpu | 0.2.0 | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| walkdir | 2.5.0 | Unlicense OR MIT | https://github.com/BurntSushi/walkdir |
| web-time | 1.1.0 | MIT OR Apache-2.0 | https://github.com/daxpedda/web-time |
| webview2-com | 0.39.1 | MIT | https://github.com/wravery/webview2-rs |
| webview2-com-macros | 0.8.1 | MIT | https://github.com/wravery/webview2-rs |
| webview2-com-sys | 0.39.1 | MIT | https://github.com/wravery/webview2-rs |
| weezl | 0.1.12 | MIT OR Apache-2.0 | https://github.com/image-rs/weezl |
| winapi-util | 0.1.11 | Unlicense OR MIT | https://github.com/BurntSushi/winapi-util |
| windows | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-version | 0.1.7 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| winit | 0.30.13 | Apache-2.0 | https://github.com/rust-windowing/winit |
| winnow | 1.0.4 | MIT | https://github.com/winnow-rs/winnow |
| wry | 0.57.1 | Apache-2.0 OR MIT | https://github.com/tauri-apps/wry |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| zlib-rs | 0.6.8 | Zlib | https://github.com/trifectatechfoundation/zlib-rs |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |
