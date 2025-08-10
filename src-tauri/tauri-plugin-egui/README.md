# tauri-plugin-egui

A simple way to render native `egui` UI in a Tauri-managed Window, either alongside or without a Webview.

<img width="1391" height="760" alt="example tauri app with egui" src="https://github.com/user-attachments/assets/164d6acb-9b5a-4dfe-9fc9-ac60b9a3e421" />

### Example Usage

```rust
// [1] import the necessary traits
use tauri_plugin_egui::{egui, EguiAppHandleExt, EguiPluginBuilder};

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      // [2] register the plugin as a `wry_plugin`.
      app.wry_plugin(EguiPluginBuilder::new(app.handle().to_owned()));

      // [3] make or get a Tauri `WebviewWindow` / `Window`
      Window::builder(app, "main")
        .inner_size(600.0, 400.0)
        .title("tauri-plugin-egui demo")
        .build()?;

      // [4]
      // start egui for a window with its label
      // pass in a closure that receives the egui::Context
      app.handle().start_egui_for_window(
        "main",
        Box::new(|ctx| {
          egui::CentralPanel::default()
            .show(ctx, |ui| {
              ui.heading("Hello from Egui!");
            });
        }),
      )?;

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
```

## Development Guide

This plugin tracks all the windows marked for `egui` in a thread-safe HashMap. Tauri maintains control over the windowing system, `egui` is only used to draw within them. For each "egui-marked" Tauri window, we create an egui context, a GPU surface and graphics renderer (`wgpu`). And tauri's `wry_plugin` mechanism is used to hook into the event loop and drive all the inputs, etc. that `egui` needs, like `RequestRedraw`.

Notes:
1. You can have multiple egui-powered windows in the same Tauri app.
2. We use `wgpu` as graphics backend, but can also consider adding `glow` eventually.
3. Our approach minimizes needing "soft forks", which was a big pitfall of [tauri-egui](https://github.com/tauri-apps/tauri/discussions/10089#discussion-6836749)
4. ~~TEMP: a custom tauri fork is needed atm. Effort is being made to merge these changes upstream.~~ ✅ merged in Tauri v2.7.0


```shell
# to check and verify everything
cargo check

# to run example app (needs bun)
cd examples/vanilla
bun tauri dev
```


## Goals

I have a Tauri app that needs to render UI without the webview-overhead for some cases. egui seemed like it can work well for this. There was a previous attempt at this by the Tauri team, detailed [here](https://v2.tauri.app/blog/tauri-egui-0-1/) but it has since been de-prioritized due to a large maintainance surface-area. My approach tries to minimize "fork maintainance" as much as possible.

[See also](https://github.com/clearlysid/egui-tao)

Perhaps we can eventually have this supported as an official Tauri plugin from their plugins workspace, but for now, this is a standalone plugin.


## References

- [egui + it's subcrates](https://github.com/emilk/egui)
- [official tauri plugins](https://github.com/tauri-apps/plugins-workspace)
- [tauri-egui integration (now unmaintained)](https://github.com/tauri-apps/tauri-egui)
