# Changelog

## [0.2.0](https://github.com/This-Is-NPC/eco/compare/v0.1.1...v0.2.0) (2026-10-10)


### Features

* **cli:** add window commands ([bf5536b](https://github.com/This-Is-NPC/eco/commit/bf5536b013b664e0e8993e8c0b7b30f2fc09e305))
* **cli:** print the usage spec ([66e58e8](https://github.com/This-Is-NPC/eco/commit/66e58e8e6bbb9afea2df5af8eefd5d2931a6f773))
* **cli:** ship shell completions ([f51e646](https://github.com/This-Is-NPC/eco/commit/f51e6462fea38ddeb3053b1db82217066a5e3444))
* **overlay:** run the window on plain Qt 6 ([11d9422](https://github.com/This-Is-NPC/eco/commit/11d9422a711962534f703bc55b469e574db111b8))
* **overlay:** show a source's transcription state ([02f5c9d](https://github.com/This-Is-NPC/eco/commit/02f5c9dd17436481d5066e05980a4603e213ce95))
* plain Qt 6 window, CLI spec and network resilience ([10e47f3](https://github.com/This-Is-NPC/eco/commit/10e47f3310a44ae0d0fd207f09e22cbd25f32eaa))
* **session:** report transcription down and back ([ca9b030](https://github.com/This-Is-NPC/eco/commit/ca9b0301aa3515984e24e48634a0aee962761d45))
* **window:** focus the window a call goes to ([2c2f5b5](https://github.com/This-Is-NPC/eco/commit/2c2f5b5330265968adbc120b35387a5a9576c684))


### Bug Fixes

* **assistant:** refuse a color for no one once ([bf7639b](https://github.com/This-Is-NPC/eco/commit/bf7639b834feaba9ff90b82b948ac26eff6aa16a))
* **channel:** bound the audio kept while down ([5e5e5d3](https://github.com/This-Is-NPC/eco/commit/5e5e5d36a81c273d2229cdad42802242f5ec4ce4))
* **channel:** reopen a stream until it is back ([c3e726b](https://github.com/This-Is-NPC/eco/commit/c3e726b1ea5e85db9d66dca5d5b0d60e0f0dbfa3))
* **channel:** retry a segment that failed ([fc89728](https://github.com/This-Is-NPC/eco/commit/fc89728f06a36dd32eeb289656e3f7d978381756))
* **cli:** hand window commands over before exiting ([3c9e2e1](https://github.com/This-Is-NPC/eco/commit/3c9e2e112b3595966bb9f8fb0caa5dc64cb4e51e))
* **import:** keep progress within the file ([c5e933e](https://github.com/This-Is-NPC/eco/commit/c5e933ee350dae02e9655068b217d59bce086566))
* **import:** tell a late client its stream is down ([8badc43](https://github.com/This-Is-NPC/eco/commit/8badc432ee9457ad56760687096a7d340e0e144f))
* **llm:** stop a stream at its first garbled event ([933b9cc](https://github.com/This-Is-NPC/eco/commit/933b9ccb16d1bec04b0803f6f49b13511473a8ca))
* **overlay:** keep the newest entry in view ([5b2ab7f](https://github.com/This-Is-NPC/eco/commit/5b2ab7f9def42dab602a3d5945a75221dcc9e126))
* **session:** give a failed delete its own code ([aa76bfb](https://github.com/This-Is-NPC/eco/commit/aa76bfb104da48379e852a989bfa3affa0076068))
* **session:** refuse a line command once ([d46d3bb](https://github.com/This-Is-NPC/eco/commit/d46d3bb4db28dcf3b0dc3e5902fc82e7f540d952))
* **session:** tell a late client a source is down ([a5847f1](https://github.com/This-Is-NPC/eco/commit/a5847f13f6b0f20772e705c244af8f7ed165e3c8))
* **setup:** write a downloaded model atomically ([25249f0](https://github.com/This-Is-NPC/eco/commit/25249f0e8f90de67eb9cfe767bd6c1a78f1f650f))
* **storage:** give each session its own log file ([838e4a0](https://github.com/This-Is-NPC/eco/commit/838e4a0d0cbfd0dc746c5fa1f5a69c891de01e0f))
* **stt:** bound the handshake of a stream ([f70ae64](https://github.com/This-Is-NPC/eco/commit/f70ae64064f3f5615e3ab58a35bd20a8b2eb0e10))
* **stt:** end a stalled stream so it reconnects ([ad21065](https://github.com/This-Is-NPC/eco/commit/ad21065518443094dabed03cb49cd9828b63f278))
* **window:** keep a window Hyprland cannot focus ([6060484](https://github.com/This-Is-NPC/eco/commit/6060484731f6d854981ed71d57351f9ebc96434b))
* **window:** raise the overlay above the settings ([dd2a32f](https://github.com/This-Is-NPC/eco/commit/dd2a32fd5b908e98f5a03111e22b3efc701864f0))

## [0.1.1](https://github.com/This-Is-NPC/eco/compare/v0.1.0...v0.1.1) (2026-10-09)


### Features

* **setup:** load the Hyprland rules from bindings.lua ([6d08fdf](https://github.com/This-Is-NPC/eco/commit/6d08fdf5bcf82c9cc881a3c29584a253ccfc6863))


### Bug Fixes

* **cli:** refuse arguments with control characters ([27fb406](https://github.com/This-Is-NPC/eco/commit/27fb4069dd39d44ca5a9439341a61280bc48e7a9))
* **cli:** refuse only line breaks in arguments ([e2996d8](https://github.com/This-Is-NPC/eco/commit/e2996d8f665a31f66682f3d3279c7b46c92ccadb))
* close the security review findings ([9788e0e](https://github.com/This-Is-NPC/eco/commit/9788e0e7638f4394d7b0e2da793138bb38a3a3d4))
* **config:** bind approval to the held change id ([db9790d](https://github.com/This-Is-NPC/eco/commit/db9790d99d8c0ef2663170b5b46cb78035e316a0))
* **config:** hold risky changes from other clients ([622ca6c](https://github.com/This-Is-NPC/eco/commit/622ca6c027a51ec63f1a2eb1499d09a6ac4c38cd))
* **daemon:** keep the conversation out of the journal ([dbafbdc](https://github.com/This-Is-NPC/eco/commit/dbafbdc1ac869bcc8fcd6ece0106a810860cca90))
* **import:** pass import paths to ffmpeg as files ([a4c3fcc](https://github.com/This-Is-NPC/eco/commit/a4c3fccd4a22ca9fdbc107c248275bcd72af8cd2))
* **overlay:** break every image opener in answers ([e5ade07](https://github.com/This-Is-NPC/eco/commit/e5ade078b889be9f82992b32eda2a3954dec5fa2))
* **overlay:** give a window started apart the token ([863a19b](https://github.com/This-Is-NPC/eco/commit/863a19bd6b7d9b7315864d723096a1038d4f95e8))
* **overlay:** render answers without remote images ([cc21e5b](https://github.com/This-Is-NPC/eco/commit/cc21e5bd9e8949ba70573bcc34b2a0f4c74d1b1f))
* **session:** resolve provider keys only for saved models ([f532762](https://github.com/This-Is-NPC/eco/commit/f532762a3c22489bff2912d510fa56b86d1c18bf))
* **setup:** write bindings.lua atomically with a backup ([94b2a85](https://github.com/This-Is-NPC/eco/commit/94b2a851749053af662d9af6de5025f814778d7a))
* **socket:** bound command lines and client queues ([f4214ff](https://github.com/This-Is-NPC/eco/commit/f4214ff32c8e45c4b323c18c832775ad3c10a91f))
* **socket:** skip input meter events for full queues ([278ea0d](https://github.com/This-Is-NPC/eco/commit/278ea0d355bb6e5a09a5f6a1a58bfb4af1de7048))
* **storage:** create user data files private ([f8f3cc5](https://github.com/This-Is-NPC/eco/commit/f8f3cc5da18d2b9f8eb7b435714672e2c2d15d0a))


### Miscellaneous Chores

* **release:** release the security fixes as 0.1.1 ([b518a07](https://github.com/This-Is-NPC/eco/commit/b518a072fda55561a36e401f1c451786bdaebbd1))

## 0.1.0 (2026-10-09)


### Continuous Integration

* **release:** cut releases with release-please ([919d85c](https://github.com/This-Is-NPC/eco/commit/919d85c49fa6af59ca3d47a91a1e6133ad0d9883))
