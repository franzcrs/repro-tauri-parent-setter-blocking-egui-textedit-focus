#[cfg_attr(mobile, tauri::mobile_entry_point)]
use std::sync::{Arc, Mutex};
use tauri::{Manager, RunEvent, State};
use tauri_plugin_egui::{
    egui,
    egui::{epaint::MarginF32, Color32},
    AppHandleExt,
};

struct AppState {
    input_value: Arc<Mutex<String>>,
    should_focus_text_edit: Arc<Mutex<bool>>,
    child_window_gained_focus: Arc<Mutex<bool>>,
}
impl Default for AppState {
    fn default() -> Self {
        Self {
            input_value: Arc::new(Mutex::new(String::from("default value"))),
            should_focus_text_edit: Arc::new(Mutex::new(true)),
            child_window_gained_focus: Arc::new(Mutex::new(false)),
        }
    }
}

#[tauri::command]
fn open_tauri_widnow(app_handle: tauri::AppHandle, state: State<AppState>) {
    let main_window = app_handle.get_window("main").unwrap();
    let width = 360.0;
    let height = 185.0;
    let tauri_egui_window = tauri::window::WindowBuilder::new(&app_handle, "tauri_egui_window")
        .inner_size(width, height)
        .resizable(false)
        .title("")
        .decorations(true)
        .maximizable(false)
        .minimizable(false)
        .closable(true)
        .title_bar_style(tauri_utils::TitleBarStyle::Overlay)
        .always_on_top(false)
        .focused(true)
        .shadow(true)
        // --------- THIS METHOD OR PARENT_RAW ALLOWS/BLOCKS THE FOCUS ON THE TEXT EDIT --------- //
        // .parent(&main_window)
        // .unwrap()
        .build()
        .unwrap();

    // Clone the Arc pointers so they can be moved into the closure
    let should_focus_text_edit_for_event = state.should_focus_text_edit.clone();
    let child_window_gained_focus_for_event = state.child_window_gained_focus.clone();
    tauri_egui_window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(focused) => {
            if *focused && *should_focus_text_edit_for_event.lock().unwrap() {
                println!("Input dialog gained focus");
                *should_focus_text_edit_for_event.lock().unwrap() = false;
                *child_window_gained_focus_for_event.lock().unwrap() = true;
            } else if !*focused {
                println!("Input dialog lost focus");
                *should_focus_text_edit_for_event.lock().unwrap() = true;
            }
        }
        _ => {}
    });

    let input_value = state.input_value.clone();
    let child_window_gained_focus = state.child_window_gained_focus.clone();

    app_handle
        .start_egui_for_window(
            "tauri_egui_window",
            Box::new(move |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(20.0);
                    let mut input_value_unwrap = input_value.lock().unwrap();
                    let input_response = ui.add(
                        egui::TextEdit::singleline(&mut *input_value_unwrap)
                            .desired_width(f32::INFINITY)
                            .margin(MarginF32::symmetric(3.0, 1.))
                            .background_color(Color32::from_rgb(44, 43, 40))
                            .text_color(Color32::from_rgb(221, 221, 221))
                            .lock_focus(true),
                    );
                    if *child_window_gained_focus.lock().unwrap() {
                        input_response.request_focus();
                        *child_window_gained_focus.lock().unwrap() = false;
                    }
                });
            }),
        )
        .ok();

    let input_value_for_event = state.input_value.clone();
    tauri_egui_window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { .. } => {
            let input_value_unwrap = input_value_for_event.lock().unwrap();
            println!("Input value: {}", *input_value_unwrap);
        }
        _ => {}
    });
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![open_tauri_widnow])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.wry_plugin(tauri_plugin_egui::Builder::new(app.handle().to_owned()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
