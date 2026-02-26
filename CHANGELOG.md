# Changelog

## [0.7.5](https://github.com/spiritledsoftware/crosspack/compare/v0.7.4...v0.7.5) (2026-02-26)


### Bug Fixes

* **installer:** register macOS GUI apps using bundle roots ([#64](https://github.com/spiritledsoftware/crosspack/issues/64)) ([5479fd1](https://github.com/spiritledsoftware/crosspack/commit/5479fd1d9c89df2fafcef7ea33970b5a19e11315))

## [0.7.4](https://github.com/spiritledsoftware/crosspack/compare/v0.7.3...v0.7.4) (2026-02-26)


### Bug Fixes

* **installer:** handle flattened macOS app-bundle binary paths (SPI-35) ([#62](https://github.com/spiritledsoftware/crosspack/issues/62)) ([9791946](https://github.com/spiritledsoftware/crosspack/commit/9791946ce219a38de28eac1328482371e50ad16b))

## [0.7.3](https://github.com/spiritledsoftware/crosspack/compare/v0.7.2...v0.7.3) (2026-02-25)


### Bug Fixes

* ci ([#60](https://github.com/spiritledsoftware/crosspack/issues/60)) ([8bce137](https://github.com/spiritledsoftware/crosspack/commit/8bce137b00efe1ce48b9e3840cf9cef9932f5435))

## [0.7.2](https://github.com/spiritledsoftware/crosspack/compare/v0.7.1...v0.7.2) (2026-02-25)


### Bug Fixes

* ci ([#59](https://github.com/spiritledsoftware/crosspack/issues/59)) ([14e6890](https://github.com/spiritledsoftware/crosspack/commit/14e68904e068b3be54bc6280ddcaaa5396290cd5))
* **ci:** release workflows ([#57](https://github.com/spiritledsoftware/crosspack/issues/57)) ([2311e44](https://github.com/spiritledsoftware/crosspack/commit/2311e4437d81ad05a5e215be070030dc66f5f9b3))


### Continuous Integration

* prevent release-please run cancellation on lock refresh ([#56](https://github.com/spiritledsoftware/crosspack/issues/56)) ([a2ce2b7](https://github.com/spiritledsoftware/crosspack/commit/a2ce2b7005fe9a92606fe0f44e65318f92ccfcdf))
* use app token for release workflow git operations ([#55](https://github.com/spiritledsoftware/crosspack/issues/55)) ([c76bc2e](https://github.com/spiritledsoftware/crosspack/commit/c76bc2ec0734daa7f7edd99a12b7eeb32cecf372))

## [0.7.1](https://github.com/spiritledsoftware/crosspack/compare/v0.7.0...v0.7.1) (2026-02-25)


### Documentation

* **agents:** add plans, update plans location preference ([#52](https://github.com/spiritledsoftware/crosspack/issues/52)) ([3822bcf](https://github.com/spiritledsoftware/crosspack/commit/3822bcf24b92adef2b047ad6f395ac9e49e32416))


### Continuous Integration

* refresh Cargo.lock in release workflow ([#54](https://github.com/spiritledsoftware/crosspack/issues/54)) ([fdf08d7](https://github.com/spiritledsoftware/crosspack/commit/fdf08d759f4a4596cb1ab73b5fc0f41cad33ada1))

## [0.7.0](https://github.com/spiritledsoftware/crosspack/compare/v0.6.0...v0.7.0) (2026-02-25)


### Features

* add deterministic cross-platform GUI installer support ([#49](https://github.com/spiritledsoftware/crosspack/issues/49)) ([61e01c6](https://github.com/spiritledsoftware/crosspack/commit/61e01c696829fb619dcedb367451d95bf04f2610))

## [0.6.0](https://github.com/spiritledsoftware/crosspack/compare/v0.5.0...v0.6.0) (2026-02-24)


### Features

* **gui:** add managed GUI app install lifecycle ([#48](https://github.com/spiritledsoftware/crosspack/issues/48)) ([542b8d9](https://github.com/spiritledsoftware/crosspack/commit/542b8d917d8754ebb43986848292d15311c62ce8))


### Bug Fixes

* **cli:** initialize zsh completion system before registration ([#46](https://github.com/spiritledsoftware/crosspack/issues/46)) ([ee4c226](https://github.com/spiritledsoftware/crosspack/commit/ee4c2266356939177b2099b288728d9513ac5fc9))

## [0.5.0](https://github.com/spiritledsoftware/crosspack/compare/v0.4.1...v0.5.0) (2026-02-24)


### Features

* **cli:** add automatic rich lifecycle output ([#44](https://github.com/spiritledsoftware/crosspack/issues/44)) ([4a8ed42](https://github.com/spiritledsoftware/crosspack/commit/4a8ed42494d691106393195aa6acc30f3fc1b39b))

## [0.4.1](https://github.com/spiritledsoftware/crosspack/compare/v0.4.0...v0.4.1) (2026-02-24)


### Bug Fixes

* **cli:** allow self-update to replace current binary ([#42](https://github.com/spiritledsoftware/crosspack/issues/42)) ([ac2a598](https://github.com/spiritledsoftware/crosspack/commit/ac2a5981387c14b540f4f17ffe1b029b97709e11))

## [0.4.0](https://github.com/spiritledsoftware/crosspack/compare/v0.3.1...v0.4.0) (2026-02-24)


### Features

* **cli:** add self-update command ([#40](https://github.com/spiritledsoftware/crosspack/issues/40)) ([8897184](https://github.com/spiritledsoftware/crosspack/commit/8897184b4eb24c0c6e727366722abe1aa5a2d868))

## [0.3.1](https://github.com/spiritledsoftware/crosspack/compare/v0.3.0...v0.3.1) (2026-02-24)


### Bug Fixes

* **release:** wait for release checksums before registry sync ([#38](https://github.com/spiritledsoftware/crosspack/issues/38)) ([dcedeb6](https://github.com/spiritledsoftware/crosspack/commit/dcedeb6772cfb8aeacf1f96da5037baffe25db5e))

## [0.3.0](https://github.com/spiritledsoftware/crosspack/compare/v0.2.1...v0.3.0) (2026-02-24)


### Features

* **release:** sync stable releases into registry index ([#36](https://github.com/spiritledsoftware/crosspack/issues/36)) ([cc9a164](https://github.com/spiritledsoftware/crosspack/commit/cc9a1647dc100fa703dba3141e37e338e125ef0f))

## [0.2.1](https://github.com/spiritledsoftware/crosspack/compare/v0.2.0...v0.2.1) (2026-02-24)


### Bug Fixes

* **ci:** unblock release builds after version bumps ([#34](https://github.com/spiritledsoftware/crosspack/issues/34)) ([fb79b3c](https://github.com/spiritledsoftware/crosspack/commit/fb79b3c794b627ff29e045cc93cdba4facf7a963))

## [0.2.0](https://github.com/spiritledsoftware/crosspack/compare/v0.1.0...v0.2.0) (2026-02-24)


### Features

* add binary exposure with ownership collision checks ([9e7fc34](https://github.com/spiritledsoftware/crosspack/commit/9e7fc346a59abe9227011097b579647a716df722))
* add dependency-aware uninstall with orphan pruning ([b544d0a](https://github.com/spiritledsoftware/crosspack/commit/b544d0a9f000598b6332beb61e808b691d6987c0))
* add graph-based dependency resolution for install and upgrade ([4bbe66c](https://github.com/spiritledsoftware/crosspack/commit/4bbe66c58b496d82a21bf88a83d44b9e5b532f5c))
* add provider override parsing + policy info output (SPI-16) ([#5](https://github.com/spiritledsoftware/crosspack/issues/5)) ([3589fe5](https://github.com/spiritledsoftware/crosspack/commit/3589fe5535dc5a1b4f197d0f05198f0a72b4d360))
* add rollback command path checkpoint (SPI-17) ([#6](https://github.com/spiritledsoftware/crosspack/issues/6)) ([fe43efb](https://github.com/spiritledsoftware/crosspack/commit/fe43efb37995344f8dfba111f01eb869affbc350))
* add shell completions command and installer setup ([#23](https://github.com/spiritledsoftware/crosspack/issues/23)) ([a61d46a](https://github.com/spiritledsoftware/crosspack/commit/a61d46a047c239189325c3643f096448b13a8b9c))
* **cli:** add registry source management and update commands ([dd59d48](https://github.com/spiritledsoftware/crosspack/commit/dd59d4873f3bf9c9b5090fb1540deb69b7afc2c3))
* **cli:** add version command and --version flag ([#27](https://github.com/spiritledsoftware/crosspack/issues/27)) ([a436ceb](https://github.com/spiritledsoftware/crosspack/commit/a436ceb764e79e7194edf5f069d74dd351c57974))
* **cli:** align registry command output with source spec ([21921c4](https://github.com/spiritledsoftware/crosspack/commit/21921c488c1d5c597f60e51b65b0ca6e269b72a0))
* **cli:** improve search discoverability with filters and source-aware output ([#25](https://github.com/spiritledsoftware/crosspack/issues/25)) ([1f6dd3e](https://github.com/spiritledsoftware/crosspack/commit/1f6dd3e6aee23084d38a58ff33395b5ba646d3fd))
* **cli:** use configured snapshots for metadata by default ([cd3d60e](https://github.com/spiritledsoftware/crosspack/commit/cd3d60e3e06ddb48015f85de91f0d5ae947901e2))
* harden installer replacement flow (SPI-15) ([#4](https://github.com/spiritledsoftware/crosspack/issues/4)) ([0a64808](https://github.com/spiritledsoftware/crosspack/commit/0a64808e49c19e480198267b4f5e133c4448e4f0))
* **launch:** add snapshot mismatch monitoring and health check (SPI-21) ([#11](https://github.com/spiritledsoftware/crosspack/issues/11)) ([3ccafeb](https://github.com/spiritledsoftware/crosspack/commit/3ccafeb4409d393490103d78be48ed7bd797e587))
* **registry:** add git source synchronization for updates ([ba937b0](https://github.com/spiritledsoftware/crosspack/commit/ba937b0376deb96122c5071c34d36d55be1c3604))
* **registry:** add source configuration state management ([e103f9c](https://github.com/spiritledsoftware/crosspack/commit/e103f9c1b2f620859268262dfb67a408de7872f6))
* **registry:** enforce signed metadata for search and clarify trust anchor ([b5beb19](https://github.com/spiritledsoftware/crosspack/commit/b5beb1947c3ff94aa222038e9cfa14741b9c3e2f))
* **registry:** implement filesystem source update and snapshot verification ([12b1c9c](https://github.com/spiritledsoftware/crosspack/commit/12b1c9c9448859fdb4df9d8ae5dfd0708bd25ca2))
* **registry:** read metadata from prioritized source snapshots ([e8ed810](https://github.com/spiritledsoftware/crosspack/commit/e8ed8107ac7df262d11bd818948fc049e8ffdb91))
* **registry:** require signed metadata for manifest loading ([0bf8b1c](https://github.com/spiritledsoftware/crosspack/commit/0bf8b1c5cb9eca08bff8955ff54042db548ca01d))
* **security:** add ed25519 detached signature verification ([3f9adad](https://github.com/spiritledsoftware/crosspack/commit/3f9adad3180f0f80eb171c0a02a459d148606a06))
* **spi-22:** add tagged release workflow and artifact publishing ([#14](https://github.com/spiritledsoftware/crosspack/issues/14)) ([e4ec481](https://github.com/spiritledsoftware/crosspack/commit/e4ec48144ab44f55755aaed6656831ac0c0e97b8))
* **spi-33:** add dry-run diff + transaction preview for install/upgrade ([#24](https://github.com/spiritledsoftware/crosspack/issues/24)) ([f28bf58](https://github.com/spiritledsoftware/crosspack/commit/f28bf58f3c164af390f6717b5031d507cbf34c62))
* **SPI-7:** kickoff artifact + unblock plan ([#2](https://github.com/spiritledsoftware/crosspack/issues/2)) ([3bee118](https://github.com/spiritledsoftware/crosspack/commit/3bee118e40b9b4d29aa41d6a8efb59397a8be978))
* **SPI-8:** transaction metadata + journal scaffolding ([#3](https://github.com/spiritledsoftware/crosspack/issues/3)) ([e69dc35](https://github.com/spiritledsoftware/crosspack/commit/e69dc3506c617b1c0ef89479f16f58e29fd0ec19))
* support multi-target global upgrade safely ([65875fd](https://github.com/spiritledsoftware/crosspack/commit/65875fd77b5722c9a21ee39d39873e875f784644))


### Bug Fixes

* **cli:** harden registry update failure handling ([5b78541](https://github.com/spiritledsoftware/crosspack/commit/5b78541c20891cf6998416974733001c67c795f9))
* **installer:** preserve symlinked shell profiles during setup ([#28](https://github.com/spiritledsoftware/crosspack/issues/28)) ([25ee42c](https://github.com/spiritledsoftware/crosspack/commit/25ee42ceef0225ec2a904ec0cb9eead22a0379b9))
* **registry:** align source state schema with v0.3 spec ([060d6f5](https://github.com/spiritledsoftware/crosspack/commit/060d6f58fa509de11303e2f9a98ae9484f2d99b8))
* **registry:** format git snapshot ids with source prefix ([0f8200a](https://github.com/spiritledsoftware/crosspack/commit/0f8200a3fc9883513df2928e14ee98ac6c893a24))
* **registry:** harden snapshot change detection and rollback errors ([71cbf81](https://github.com/spiritledsoftware/crosspack/commit/71cbf81687ec76e3c97b2d64d5ee05cd45d6ec24))
* **registry:** make git snapshot ids deterministic ([8fa9af7](https://github.com/spiritledsoftware/crosspack/commit/8fa9af70f337b32368db0f1d1e8156cbcf31d8f7))
* **registry:** surface configured source state read errors ([c3b96b7](https://github.com/spiritledsoftware/crosspack/commit/c3b96b77ad40b4f3ac5565293d845dbc96ba0e5f))
* **registry:** tighten source state validation and versioning ([c720062](https://github.com/spiritledsoftware/crosspack/commit/c720062cd6303fd1be220ccf0f5a2bf34f66642c))
* **registry:** validate loaded source state and enforce schema version ([4cba3c8](https://github.com/spiritledsoftware/crosspack/commit/4cba3c88381a4cd5d6970570e1e8e6b1a5aa9479))
* **spi-24:** replay rollback journal and recover interrupted transact… ([#13](https://github.com/spiritledsoftware/crosspack/issues/13)) ([f054bb8](https://github.com/spiritledsoftware/crosspack/commit/f054bb8ab81388d186f930d87bd2ac0b88ff1006))
* **zsh:** load package completions via fpath instead of sourcing files ([#26](https://github.com/spiritledsoftware/crosspack/issues/26)) ([c619818](https://github.com/spiritledsoftware/crosspack/commit/c6198182236335e28c29c16da1897498d579a83e))


### Documentation

* add generated AGENTS knowledge files ([55ca00a](https://github.com/spiritledsoftware/crosspack/commit/55ca00af8a88d6c5cc6d2719757405ea73d1fcf5))
* add registry metadata signing implementation plan ([a63c8a4](https://github.com/spiritledsoftware/crosspack/commit/a63c8a4c5e066867f1cc78e177f8a75621f49205))
* add v0.3 source management implementation plan ([b30dff7](https://github.com/spiritledsoftware/crosspack/commit/b30dff78d0108af91b2e44b903562fd60fd02396))
* add v0.3-v0.5 specs and core doc cross-links ([c9b4ceb](https://github.com/spiritledsoftware/crosspack/commit/c9b4ceb76eda4452aa418aae92f81c4a6441bb79))
* align signing paths and upgrade flow wording ([163d484](https://github.com/spiritledsoftware/crosspack/commit/163d484c3c782415d3221421a8a8cb2d29c574b4))
* clarify v0.3 does not change manifest schema ([5b8b124](https://github.com/spiritledsoftware/crosspack/commit/5b8b124248cf442b62f34d53845516c07f937703))
* codify agent workflow note-taking protocol ([3fae0ec](https://github.com/spiritledsoftware/crosspack/commit/3fae0ec3fa9d99cbd1f6dd79b21963e5e07bbdc0))
* complete SPI-19 contributor playbook and launch runbook ([#9](https://github.com/spiritledsoftware/crosspack/issues/9)) ([e546751](https://github.com/spiritledsoftware/crosspack/commit/e54675196116d3205a2b0c059e631ce5f531cff2))
* define strict registry metadata signing behavior ([4a30482](https://github.com/spiritledsoftware/crosspack/commit/4a30482d16a35f2f6286bca0a3e7210853cdf199))
* document implemented source management workflow ([3e89d59](https://github.com/spiritledsoftware/crosspack/commit/3e89d59de88dcaddfb55ac43647817f7a8e7343d))
* normalize target-group solve terminology ([9ca9d63](https://github.com/spiritledsoftware/crosspack/commit/9ca9d63d2b45690796a7abb1f416edfd2e928c15))
* **readme:** add comprehensive project overview and onboarding guide ([#10](https://github.com/spiritledsoftware/crosspack/issues/10)) ([fe5f0d6](https://github.com/spiritledsoftware/crosspack/commit/fe5f0d6d2571d85f3642ca571d0cd02ba6572996))
* **readme:** add one-liner install section for prebuilt artifacts ([#20](https://github.com/spiritledsoftware/crosspack/issues/20)) ([7ed98c5](https://github.com/spiritledsoftware/crosspack/commit/7ed98c5eb470462a9cc54cc6c305cc2b4cc9d170))
* **spi-23:** lock GA scope wording and mark roadmap specs non-GA ([#16](https://github.com/spiritledsoftware/crosspack/issues/16)) ([31855dd](https://github.com/spiritledsoftware/crosspack/commit/31855dd9b241dee68573829580c4d390a7086a1a))
* **spi-25:** reconcile architecture/install/manifest follow-through ([#22](https://github.com/spiritledsoftware/crosspack/issues/22)) ([ec1678f](https://github.com/spiritledsoftware/crosspack/commit/ec1678fe62e6a49683c208e78a9aa2bce6e390e4))
* **spi-26:** publish official registry bootstrap and trust flow ([#15](https://github.com/spiritledsoftware/crosspack/issues/15)) ([1074449](https://github.com/spiritledsoftware/crosspack/commit/1074449d561fa29a3415fb6c8bac5ccfd5272074))


### Continuous Integration

* **release:** add ARM release builds ([#18](https://github.com/spiritledsoftware/crosspack/issues/18)) ([566675b](https://github.com/spiritledsoftware/crosspack/commit/566675bfe1100820ec855015629b841d6ab5d07e))
* **release:** create GitHub releases with attached artifacts ([#17](https://github.com/spiritledsoftware/crosspack/issues/17)) ([76b9cca](https://github.com/spiritledsoftware/crosspack/commit/76b9cca7ab4c5c8d1b592f91a755be521d6d024f))
* **release:** fix macOS x64 runner label ([#19](https://github.com/spiritledsoftware/crosspack/issues/19)) ([97d7337](https://github.com/spiritledsoftware/crosspack/commit/97d7337b7dfb46850a63fcc7adaaf00af6ff282b))
* **release:** harden CI and automate release/versioning ([#29](https://github.com/spiritledsoftware/crosspack/issues/29)) ([3769dae](https://github.com/spiritledsoftware/crosspack/commit/3769daef32713d31408b68017c6687be493fdf64))
* **spi-27:** enforce snapshot-flow validation in workflow ([#12](https://github.com/spiritledsoftware/crosspack/issues/12)) ([f9fb8ea](https://github.com/spiritledsoftware/crosspack/commit/f9fb8eae760cf4af81ce0e2e2e3d8555fec5ec72))

## Changelog

All notable changes to Crosspack will be documented in this file.

This file is maintained by Release Please from Conventional Commit history.

## Unreleased

### Documentation

- Migrate project licensing metadata and docs to dual-license `MIT OR Apache-2.0`.
