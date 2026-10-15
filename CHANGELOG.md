# Changelog

## [0.6.0](https://github.com/This-Is-NPC/eco/compare/v0.5.1...v0.6.0) (2026-10-15)


### ⚠ BREAKING CHANGES

* **timeline:** session summaries (`eco sessions`, `eco show`) carry `counts` {lines, answers, notes} instead of `speech` and `suggestions`.

### Features

* **lines:** merge and split lines from a line menu ([652335b](https://github.com/This-Is-NPC/eco/commit/652335bd99383209c4b0b2763ec67448b149dfae))
* **socket:** let a client leave out event types ([9e52a93](https://github.com/This-Is-NPC/eco/commit/9e52a93894f5b4873acdeb4963179c6c3104cff2))
* **timeline:** open a line's menu from a visible button ([54cf353](https://github.com/This-Is-NPC/eco/commit/54cf3534c96309b65cdf76e1507256a77d2f90ed))


### Bug Fixes

* **details:** lay the panel out before scrolling to the speakers ([73c1643](https://github.com/This-Is-NPC/eco/commit/73c16431720b53ea6716cf103f7bdd99135d2b82))
* **diarization:** start a speaker at 0.7, as measured ([4bd454e](https://github.com/This-Is-NPC/eco/commit/4bd454e6787f9a26c51e293f8f3aaccbaa2dc092))
* **overlay:** disable what needs a chat model instead of hiding it ([fb8ef6c](https://github.com/This-Is-NPC/eco/commit/fb8ef6c98c851e4caa202de64f5172710203f611))
* **overlay:** give a view made when shown the keyboard ([bf17e39](https://github.com/This-Is-NPC/eco/commit/bf17e390ba7a7aebd436f3f50830eb0dc5d1184e))
* **search:** find a session its log's title names ([8101fde](https://github.com/This-Is-NPC/eco/commit/8101fde9a62fb994fd830b6f7c69b5637da1cd8e))
* **sessions:** index a log whose first record has no id ([a9e070c](https://github.com/This-Is-NPC/eco/commit/a9e070c4e922941cb81dce1c7c43c5b99a70d306))
* **sessions:** read a stored time back exactly ([52c173f](https://github.com/This-Is-NPC/eco/commit/52c173fa3f146c1a37ee710ae7928627aa5d57e4))
* **settings:** say a skill has no chat model ([9232968](https://github.com/This-Is-NPC/eco/commit/9232968296a59714355bb46b6779bbc7fc8cfea7))
* **timeline:** ask for earlier entries under the header ([d33e0a2](https://github.com/This-Is-NPC/eco/commit/d33e0a2226f7147890812102e9fd86b557d401fb))
* **timeline:** count a split's offset in characters ([aa9d322](https://github.com/This-Is-NPC/eco/commit/aa9d322879c2a2ed4f181bbd3004cb72a9def4bf))
* **timeline:** drop the line button's tip while its menu is open ([b94e395](https://github.com/This-Is-NPC/eco/commit/b94e395b1ce598da9b2072d1a820821c857d2ed9))
* **timeline:** keep the place read when an older page goes in ([5a61ce9](https://github.com/This-Is-NPC/eco/commit/5a61ce90914bc3d623b5333eedc071eff15ecde8))
* **timeline:** name the kind an answer cannot be translated in ([b684f31](https://github.com/This-Is-NPC/eco/commit/b684f3155d24f3c5932b31c2aa3c3dfe220e4c24))
* **timeline:** open a line's menu from its button, level with the line ([adfd620](https://github.com/This-Is-NPC/eco/commit/adfd6201d399ee450ecfab8886e06eb59bac3998))


### Performance Improvements

* **diarization:** tell live speakers apart as they speak ([9698baf](https://github.com/This-Is-NPC/eco/commit/9698baf0512a1a3e148f94fa07367e880692fab3))
* **overlay:** make views and settings only while shown ([5d41da4](https://github.com/This-Is-NPC/eco/commit/5d41da423074b2073e44a9c80114b7f612057de9))
* **service:** give freed memory back to the system ([31ffc73](https://github.com/This-Is-NPC/eco/commit/31ffc7357d2364b3f65d2034d75a7804662a96a5))
* **sessions:** read stored sessions from an index ([ac1b000](https://github.com/This-Is-NPC/eco/commit/ac1b000248604a5b9d620a5c1730e8ae83c2d566))
* **timeline:** keep the row index when the oldest rows leave ([b02e88f](https://github.com/This-Is-NPC/eco/commit/b02e88f15e38c8f62417f3d640137a78236a5ae6))
* **timeline:** read long sessions in pages ([1b0c693](https://github.com/This-Is-NPC/eco/commit/1b0c6933b7be23d86824e6e3ccdf7875f3bb58b5))
* **timeline:** take session counts from the daemon ([43c82a3](https://github.com/This-Is-NPC/eco/commit/43c82a38c593d89917a89f75c0a028c65ad13be8))

## [0.5.1](https://github.com/This-Is-NPC/eco/compare/v0.5.0...v0.5.1) (2026-10-14)


### Bug Fixes

* **packaging:** depend on the Qt the window was built with ([e80537f](https://github.com/This-Is-NPC/eco/commit/e80537f369d46f5d6c810599cf8c33b374e7b578))
* **packaging:** depend on the Qt the window was built with ([9808912](https://github.com/This-Is-NPC/eco/commit/9808912bd055185c27ebca9581cb57a699287c80))

## [0.5.0](https://github.com/This-Is-NPC/eco/compare/v0.4.0...v0.5.0) (2026-10-14)


### Features

* **chat:** make the chat model optional ([e0ee546](https://github.com/This-Is-NPC/eco/commit/e0ee546f57dfc327319b746b603c8ddf46bf9c27))
* **keys:** list key names to pick from ([0e7ebf3](https://github.com/This-Is-NPC/eco/commit/0e7ebf327e6b4cc4c40098b7ce02fd98045fec86))
* **notifications:** name Windows toasts eco with its icon ([156f226](https://github.com/This-Is-NPC/eco/commit/156f226d83230bf531f194ec6145d1d45b5cb31e))
* **notifications:** stacked toasts and system notifications ([1fe546a](https://github.com/This-Is-NPC/eco/commit/1fe546a323bed0514403a469cc1503070bf4b5fc))
* optional chat, settings rework, updates and notifications ([d8789bc](https://github.com/This-Is-NPC/eco/commit/d8789bc95a223ff8b50df04ff6468ed1ebb4c652))
* **settings:** devices, prompts and a general tab ([e05da8c](https://github.com/This-Is-NPC/eco/commit/e05da8c906ad53cc4e3274a172c2b5e394a03ba4))
* **update:** check for releases and update in place ([41b0231](https://github.com/This-Is-NPC/eco/commit/41b02310be090d3c8b874a2b064cbce80c2a2aca))


### Bug Fixes

* **installer:** wait for a file held open for a moment ([be2c853](https://github.com/This-Is-NPC/eco/commit/be2c853774382f5bd0b77b09830c9508eaeec782))
* **overlay:** clearer bars, context tab, cost at top ([2feaf19](https://github.com/This-Is-NPC/eco/commit/2feaf19c5faca356ba842d83b2ab05e3bb0015a1))
* **overlay:** keep texts a reloaded language pack lacks ([de28b61](https://github.com/This-Is-NPC/eco/commit/de28b61dc5415e501d68951bc676dc2282ddbe1a))
* **overlay:** let a new model choose its type ([42a3b21](https://github.com/This-Is-NPC/eco/commit/42a3b21630db09504accec2b2d581d55535090ea))
* **scripts:** read docs and QML as UTF-8 ([589910b](https://github.com/This-Is-NPC/eco/commit/589910be62435f6ba0015011e592b77e192989b7))
* **settings:** defaults on models, tables everywhere ([91935eb](https://github.com/This-Is-NPC/eco/commit/91935ebe8222a264fa1d1a898ad2be051db8bb8e))
* **settings:** drop inline lists, fix bar and rail ([0e29230](https://github.com/This-Is-NPC/eco/commit/0e292306080b053747c84552af39046ad80aac33))
* **window:** count a window active only in front ([5ffba7f](https://github.com/This-Is-NPC/eco/commit/5ffba7f75d1d0f46cb1c5fac6fcb7e2025f40883))
* **window:** deploy QtTest beside eco-window on windows ([6e5e26f](https://github.com/This-Is-NPC/eco/commit/6e5e26f428813aef1ad6040460a762129dd8b539))

## [0.4.0](https://github.com/This-Is-NPC/eco/compare/v0.3.0...v0.4.0) (2026-10-11)


### Features

* install eco on windows from a link ([c8254e0](https://github.com/This-Is-NPC/eco/commit/c8254e09f2d39129864fdd002ecddd8f201b957f))
* **install:** install eco on windows from a link ([8203982](https://github.com/This-Is-NPC/eco/commit/8203982360fa049292a24d54d85119d24b5642b0))

## [0.3.0](https://github.com/This-Is-NPC/eco/compare/v0.2.0...v0.3.0) (2026-10-11)


### Features

* **audio:** capture audio on windows with wasapi ([e8d22e3](https://github.com/This-Is-NPC/eco/commit/e8d22e34416e3cca598495553d498338984d8267))
* **config:** start in the system language ([b88f37b](https://github.com/This-Is-NPC/eco/commit/b88f37be3a2f548361807204780a69244408d702))
* **config:** start without a config file ([c58d569](https://github.com/This-Is-NPC/eco/commit/c58d569fe0ad75b0672a3ade258d44719c5267e7))
* **daemon:** build and test eco on windows ([a9fb496](https://github.com/This-Is-NPC/eco/commit/a9fb4966907514a4382fb39759aead7d5afad8a2))
* **hooks:** run hooks through cmd on windows ([c221cc1](https://github.com/This-Is-NPC/eco/commit/c221cc1e07821d7ce9a0900cacade2dc8c5f283b))
* **install:** install and uninstall on windows ([8e47a3e](https://github.com/This-Is-NPC/eco/commit/8e47a3e8b66e0444af9a0f33235dc675eb95a789))
* **keys:** keep keys in Credential Manager ([84733e2](https://github.com/This-Is-NPC/eco/commit/84733e2c632adad16e1fcda6d915194d650a4e81))
* run eco on windows ([f3957fc](https://github.com/This-Is-NPC/eco/commit/f3957fcd6956663cb2958c2b483e17bf9a66ba6a))
* **service:** run the daemon detached on windows ([bec3bbc](https://github.com/This-Is-NPC/eco/commit/bec3bbcae8b2c4407db252ad9674e16965f1be17))
* **setup:** add eco to the Start menu on windows ([dee89aa](https://github.com/This-Is-NPC/eco/commit/dee89aa75a7744377f96b21e2e4e2ec4bee78c1e))
* **shortcuts:** register global keys on windows ([a1cd906](https://github.com/This-Is-NPC/eco/commit/a1cd90622f897cec008e5ef465c2f0befca0e5bb))
* **tray:** show eco in the tray when enabled ([a1ed332](https://github.com/This-Is-NPC/eco/commit/a1ed332d32f5365260558b5c319ff317c354e726)), closes [#3235](https://github.com/This-Is-NPC/eco/issues/3235)
* **window:** control eco's windows on windows ([d428a48](https://github.com/This-Is-NPC/eco/commit/d428a48e68e60f75d5f58c46d89f18a6d23e7cfe))
* **window:** run the eco window on windows ([1a1d0e0](https://github.com/This-Is-NPC/eco/commit/1a1d0e0f5b7cfc3975af66dc92fa1d5432570c16))


### Bug Fixes

* **audio:** tell the settings if echo cancels here ([ff64211](https://github.com/This-Is-NPC/eco/commit/ff64211256f7831a389fc6816688957acbd45108))
* **capture:** stop only the device that fails ([b62c031](https://github.com/This-Is-NPC/eco/commit/b62c031d84a41c6169f179bbedb0701f468014bc))
* **card:** stop the wait count at the first word ([b5d339e](https://github.com/This-Is-NPC/eco/commit/b5d339ea3419a1ab4f2705866e18f36c658c3244))
* **channel:** open no stream for an unheard source ([790035f](https://github.com/This-Is-NPC/eco/commit/790035f2b3132b52ba29e89b69cbe94b1d354bc0))
* **cost:** count only the inputs heard ([7e066bd](https://github.com/This-Is-NPC/eco/commit/7e066bdfaaae21878bd2da419a3bee0171bb8cad))
* **devices:** name the default devices in english ([a1196e1](https://github.com/This-Is-NPC/eco/commit/a1196e1c031f69809c1cb9288a478c49263d1669))
* **import:** report a broken ffprobe as missing ([c655b48](https://github.com/This-Is-NPC/eco/commit/c655b4828f9dd3a26b6ef06ffebc0b0d55764b0b))
* **import:** say when ffmpeg is missing ([bfcbb16](https://github.com/This-Is-NPC/eco/commit/bfcbb1649c59c8e823e313abeb7abfb7892a56d8))
* **overlay:** clear live errors when sessions end ([eefd0cb](https://github.com/This-Is-NPC/eco/commit/eefd0cb38b96c0c10f5d2a818cabb418da89fe3e))
* **overlay:** describe screen-share hiding on both ([4c4a86b](https://github.com/This-Is-NPC/eco/commit/4c4a86b26869fb32f9d7867285568f2959f6e1bf))
* **overlay:** end a window that closed but stays ([6116fe9](https://github.com/This-Is-NPC/eco/commit/6116fe927eb4a3f8826097296bb9ede3fca08aad))
* **paths:** build shipped paths with one separator ([9ef3c7e](https://github.com/This-Is-NPC/eco/commit/9ef3c7e5b2d9f49106a6383aca324cb01107d904))
* **service:** return from eco start on windows ([ba90954](https://github.com/This-Is-NPC/eco/commit/ba9095421ab20444e219bbb274f548e5b0313836))
* **service:** say why the daemon did not start ([3922084](https://github.com/This-Is-NPC/eco/commit/3922084551041f381ec889ef7ea5cc1cf673a86b))
* **settings:** save a draft that names no model ([c10af39](https://github.com/This-Is-NPC/eco/commit/c10af396f3e1b945da58e8bf61fc5b277b1b8fae))
* **settings:** title the window in english ([c5b2d99](https://github.com/This-Is-NPC/eco/commit/c5b2d99919fef1f6aad33445524ee37aba5e386d))
* **setup:** never start eco at Windows sign-in ([c1ff6b8](https://github.com/This-Is-NPC/eco/commit/c1ff6b89b83de95c388862be58186948ab3e4fe5))
* **shortcuts:** pick free ask and probe keys ([54654af](https://github.com/This-Is-NPC/eco/commit/54654af722e92f81f2d7740c7be189d471874767))
* **socket:** require the token on the windows pipe ([22893ce](https://github.com/This-Is-NPC/eco/commit/22893cefe5b8730e152d818a9d8a2e13e627082b))
* **start:** bring the open window forward ([2d959d6](https://github.com/This-Is-NPC/eco/commit/2d959d66c2d962c61915ea52e757dba613d92cd6))
* **start:** restart a daemon of another version ([4b03068](https://github.com/This-Is-NPC/eco/commit/4b030685093ee93d386402de3a27149b1090edad))
* **tray:** open the tray menu on windows ([24ed1af](https://github.com/This-Is-NPC/eco/commit/24ed1af944fb8fea9d9b001bab7ac3ee10fbe56e))
* **window:** end the process when it closes ([6d87e9a](https://github.com/This-Is-NPC/eco/commit/6d87e9a7ee8b74cd20e2ca11d8a10ec6ee73eacb))
* **window:** keep a copied text after eco stops ([1a9b197](https://github.com/This-Is-NPC/eco/commit/1a9b197785f0978f5102e9ab4cca75852ea699ee))
* **window:** run eco window on windows ([3cf1a1a](https://github.com/This-Is-NPC/eco/commit/3cf1a1af64e2c15419b31e2f14bf8511c56fcebf))
* **window:** take the foreground on windows ([93eb29d](https://github.com/This-Is-NPC/eco/commit/93eb29d4dc6da93c11950afe0991ef0681202338))

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
