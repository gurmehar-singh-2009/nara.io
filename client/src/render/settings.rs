//! Options from the settings drawer in index.html. The page saves them to localStorage; the game
//! reads them at startup and again whenever a control in the page changes.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::render::colours::set_light_mode;

/// "light" or "dark". Anything else is dark, the menu's default.
const THEME_KEY: &str = "diep10-theme";
/// "on" or "off". Missing means on.
const GRID_KEY: &str = "nara.grid";
/// "webgpu" or "webgl2". Missing means WebGL2, the menu's default. Only read at startup:
/// switching reloads the page.
const RENDERER_KEY: &str = "nara.renderer";
/// "on" or "off". Missing means on.
const FPS_KEY: &str = "nara.fps";
/// "on" or "off". Missing means off. Renders at one pixel per CSS pixel on high-DPI screens.
const LOW_DETAIL_KEY: &str = "nara.lowdetail";
/// "on" or "off". Missing means on. Picks the GPU power preference; read only at startup.
const HW_ACCEL_KEY: &str = "nara.hwaccel";

static SHOW_GRID: AtomicBool = AtomicBool::new(true);
static SHOW_FPS: AtomicBool = AtomicBool::new(true);
static LOW_DETAIL: AtomicBool = AtomicBool::new(false);
/// Set when low detail changes, so the renderer resizes its surface.
static RESIZE_REQUEST: AtomicBool = AtomicBool::new(false);

fn read(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()
        .flatten()?
        .get_item(key)
        .ok()
        .flatten()
}

pub fn sync_from_page() {
    set_light_mode(read(THEME_KEY).as_deref() == Some("light"));
    SHOW_GRID.store(read(GRID_KEY).as_deref() != Some("off"), Ordering::Relaxed);
    SHOW_FPS.store(read(FPS_KEY).as_deref() != Some("off"), Ordering::Relaxed);

    let low_detail = read(LOW_DETAIL_KEY).as_deref() == Some("on");

    if LOW_DETAIL.swap(low_detail, Ordering::Relaxed) != low_detail {
        RESIZE_REQUEST.store(true, Ordering::Relaxed);
    }
}

pub fn show_grid() -> bool {
    SHOW_GRID.load(Ordering::Relaxed)
}

pub fn show_fps() -> bool {
    SHOW_FPS.load(Ordering::Relaxed)
}

pub fn low_detail() -> bool {
    LOW_DETAIL.load(Ordering::Relaxed)
}

/// True once after low detail was switched.
pub fn take_resize_request() -> bool {
    RESIZE_REQUEST.swap(false, Ordering::Relaxed)
}

pub fn prefers_webgpu() -> bool {
    read(RENDERER_KEY).as_deref() == Some("webgpu")
}

pub fn power_preference() -> wgpu::PowerPreference {
    if read(HW_ACCEL_KEY).as_deref() == Some("off") {
        wgpu::PowerPreference::LowPower
    } else {
        wgpu::PowerPreference::HighPerformance
    }
}

/// Writes into an element of the settings drawer, if the page has it.
pub fn set_page_text(id: &str, text: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    {
        el.set_text_content(Some(text));
    }
}
