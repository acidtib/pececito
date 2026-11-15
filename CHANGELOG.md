# Changelog

## [1.0.0](https://github.com/acidtib/pececito/compare/v0.3.0...v1.0.0) (2026-11-15)


### ⚠ BREAKING CHANGES

* **addon:** the app no longer reads the first release's three squares. Update the addon from the app and type /reload in the game.

### Features

* **addon:** send data to the app as a strip of three-bit cells ([3f12ed7](https://github.com/acidtib/pececito/commit/3f12ed76ed6dd088e741e585af181638fde70396)), closes [#12](https://github.com/acidtib/pececito/issues/12)
* pause fishing when the player moves ([0122ad2](https://github.com/acidtib/pececito/commit/0122ad20271e6288f83d440b767d9b8ec6d2dbbb)), closes [#16](https://github.com/acidtib/pececito/issues/16)
* **pececito:** check for and install new releases from settings ([9640d1a](https://github.com/acidtib/pececito/commit/9640d1a68e7480bd26e3fe4a2a8bf3595fa52f58)), closes [#17](https://github.com/acidtib/pececito/issues/17)
* **pececito:** count the cast down from 30s ([82a8201](https://github.com/acidtib/pececito/commit/82a820199fcf69e538f061589d0cb7b4e084ee82)), closes [#13](https://github.com/acidtib/pececito/issues/13)
* show the addon version in settings and offer update only for an older addon ([b23b51a](https://github.com/acidtib/pececito/commit/b23b51a3e24dbcfd0ce967f7a7861b3aad45d776)), closes [#14](https://github.com/acidtib/pececito/issues/14)
* show what each hook caught in recent casts ([5841c0a](https://github.com/acidtib/pececito/commit/5841c0a06b812b7c858212ff91a92c8f18d8d2c8)), closes [#15](https://github.com/acidtib/pececito/issues/15)


### Bug Fixes

* **fishing:** recast after a missed bite and follow a hand cast while looting ([a5da3b5](https://github.com/acidtib/pececito/commit/a5da3b534b933a8de89b69236cf1d84843bb287a))
* fit long errors in the status card ([9c6db9a](https://github.com/acidtib/pececito/commit/9c6db9aa5158715ff7fd30fabe9936b1ff3ae521))
* **game:** keep fishing when another window closes during a scan ([20bf0be](https://github.com/acidtib/pececito/commit/20bf0be5407ce816c236204f16f8e777d3b400be))
* keep the installed app when an update fails to unpack ([8bb4870](https://github.com/acidtib/pececito/commit/8bb4870cad5f5b9025febcf8ca4811397fdc0995))
* **pececito:** fill the wait bar as the cast goes on ([a8c8f60](https://github.com/acidtib/pececito/commit/a8c8f605bfbabdbdef0aacf7e6a7ac1c277129e5))
* **pececito:** show a hooked cast once its loot lands ([779d644](https://github.com/acidtib/pececito/commit/779d6447ee164805d0db8809ebcc9179900952fe))
* say finding the game isn't supported on windows and open the settings file there ([ae493da](https://github.com/acidtib/pececito/commit/ae493dab3d13d46f14f2d05307fd055443a7f15b))
* tell an outdated addon and an unreadable game window from an unloaded addon ([4eedb91](https://github.com/acidtib/pececito/commit/4eedb916bfd4d1edbed24af6bb568c1c68c90dff))

## [0.3.0](https://github.com/acidtib/pececito/compare/v0.2.0...v0.3.0) (2026-11-15)


### Features

* install the latest linux release with a curl script ([6268f56](https://github.com/acidtib/pececito/commit/6268f564c946d1721ef97ff8b6039241ca324269))

## [0.2.0](https://github.com/acidtib/pececito/compare/v0.1.0...v0.2.0) (2026-11-15)


### Features

* fish by listening for the splash and pressing the fishing key ([876dc4d](https://github.com/acidtib/pececito/commit/876dc4d183abff9dee44ae36a442ca5d8a656be4))
* have KWin remember the window position on KDE Wayland ([7e1f02f](https://github.com/acidtib/pececito/commit/7e1f02f7e629585b28d74df2ca74271ba150373b))
* **pececito:** give the windows build an icon and no console window ([fb97871](https://github.com/acidtib/pececito/commit/fb978714c04be7d3c054126ea9a6664c418a5a9d))
* **pececito:** pick how long to fish next to start and open the settings file ([5636c2a](https://github.com/acidtib/pececito/commit/5636c2a574424093932770f54cff37f280dc5083))
* **pececito:** show the logo, name and version at the bottom of settings ([dc289fe](https://github.com/acidtib/pececito/commit/dc289fe21b3083f0ab34c93d3888012309f9ae0f))


### Bug Fixes

* **pececito:** show the hand cursor over clickable controls ([fd67d13](https://github.com/acidtib/pececito/commit/fd67d13ce4307bfbdc47827be7d016abd35524ea))
