use gpui::{
    App, AppContext, Application, Bounds, Context, IntoElement, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use spqx_gui_services_probe::{Selection, enqueue};
use std::sync::{
    OnceLock,
    mpsc::{SyncSender, sync_channel},
};
use std::time::Duration;

static SENDER: OnceLock<SyncSender<Selection>> = OnceLock::new();

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn spqx_probe_register(callback: extern "C" fn(*const u8, usize) -> i32);
    fn spqx_probe_exercise() -> i32;
}

#[cfg(target_os = "macos")]
extern "C" fn receive_selection(bytes: *const u8, len: usize) -> i32 {
    if bytes.is_null() || len > spqx_gui_services_probe::MAX_SELECTION_BYTES {
        return 0;
    }
    // Pointer/length originate from our synchronous ObjC shim's NSString.
    // Copy before returning; never unwind over the foreign frame.
    std::panic::catch_unwind(|| {
        let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
        SENDER
            .get()
            .is_some_and(|sender| enqueue(sender, bytes).is_ok()) as i32
    })
    .unwrap_or(0)
}

struct Probe {
    accepted: usize,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Unstyled feasibility view, not a proposed product screen.
        div()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child("SPQX native Services probe")
            .child("No speech engine connected. No selection is saved.")
            .child(format!("Accepted selections: {}", self.accepted))
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let exercise = args.iter().any(|arg| arg == "--exercise-service-handler");
    let receipt = args
        .windows(2)
        .find(|pair| pair[0] == "--receipt")
        .map(|pair| std::path::PathBuf::from(&pair[1]));
    let (sender, receiver) = sync_channel(1);
    SENDER.set(sender).expect("one application instance");
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(480.), px(180.)), cx);
        let window = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Probe { accepted: 0 }),
        ).expect("open probe window");
        cx.on_window_closed(|cx| cx.quit()).detach();
        #[cfg(target_os = "macos")]
        unsafe {
            spqx_probe_register(receive_selection);
            if exercise {
                assert_ne!(spqx_probe_exercise(), 0, "owned service handler exercise");
            }
        }
        #[cfg(not(target_os = "macos"))]
        if exercise {
            enqueue(SENDER.get().unwrap(), b"Synthetic portable handoff").unwrap();
        }
        cx.activate(true);
        eprintln!("SPQX probe: native application loop ready; no speech engine");
        cx.spawn(async move |cx| {
            let mut accepted = 0usize;
            let mut ticks = 0usize;
            loop {
                while receiver.try_recv().is_ok() {
                    accepted += 1;
                }
                let running = cx.update(|cx| {
                    window.update(cx, |view, _window, cx| {
                        view.accepted = accepted;
                        cx.notify();
                    }).is_ok()
                }).unwrap_or(false);
                ticks += 1;
                if exercise && ticks >= 3 {
                    assert_eq!(accepted, 1, "exactly one synthetic request");
                    if let Some(path) = &receipt {
                        std::fs::write(path, format!(
                            "{{\"platform\":\"{}\",\"gpui_application_loop\":true,\"accepted\":{},\"os_services_routing_tested\":false,\"inference\":false}}\n",
                            std::env::consts::OS, accepted
                        )).expect("write receipt");
                    }
                    let _ = cx.update(|cx| cx.quit());
                    break;
                }
                if !running { break; }
                cx.background_executor().timer(Duration::from_millis(120)).await;
            }
        }).detach();
    });
}
