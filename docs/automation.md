# Native inspection and scripted input

Startpoint uses Rune's built-in tooling. The former GPUI automation feature and external driver are removed.

```sh
cargo build --locked
mkdir -p target/interface
target/debug/startpoint --scene library --snap target/interface/library.png --tree target/interface/library.json
target/debug/startpoint --scene journal --size 900x640 --snap target/interface/journal.png
target/debug/startpoint --scene component-effects --snap target/interface/effects.png
target/debug/startpoint --list-scenes
target/debug/startpoint --help
```

Scenes include Library, Activity, Settings, editor sections, Journal, customization, manual addition, commands, notifications, compact Table, Infinite Wall and Component effects. With no scene, the example starts on its normal screen. Headless tools always use a fresh temporary example database, without OS credentials, remote artwork providers, external opening or real game launches.

`--script FILE.json` replays timestamped input through the same input handlers as the window. Address nested fields by scoped keys such as `editor-title/value/input`, `journal-text/input`, `task-text/value/input` and `effect-input/value/input`. `type` takes virtual time per character; use `paste` or leave enough time before pressing Save. Select options use scoped `option0`, `option1`, etc. keys beneath their popup.

```json
{
  "events": [
    { "t": 0.2, "type": "click", "key": "effect-button" },
    { "t": 0.5, "type": "click", "key": "effect-pause" }
  ]
}
```

Use `--frames`, `--fps`, `--at`, `--trace`, `--video`, `--out`, `--strip` and `--crop` for deterministic rendering and motion inspection. Frame dumps and offscreen throughput are not native monitor FPS measurements. Headless image handling waits for decoding, so asynchronous image wakeups still need interactive acceptance.

`--native` can record actual submissions. Native scripted input has not been validated, so do not interpret a native trace as proof that a scripted click ran. Interactive sessions can enable developer samples with `--developer-tools`; normal release sessions keep them hidden.
