use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextPattern2,
    UIA_TextPattern2Id, UIA_TextPatternId,
};

/// Initializes MTA COM on the current platform worker thread.
/// Safe to call even if COM was already initialized on this thread.
pub fn init_mta_com() {
    // SAFETY: Standard thread-local COM initialization for the dedicated platform worker thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        log::debug!("CoInitializeEx(COINIT_MULTITHREADED) returned {hr:?}");
    }
}

/// Holds a reusable `IUIAutomation` instance bound to the MTA COM worker thread.
pub struct UiaReader {
    automation: Option<IUIAutomation>,
}

impl UiaReader {
    pub fn new() -> Self {
        // SAFETY: Called on the MTA COM worker thread after `init_mta_com()`.
        let automation = unsafe {
            CoCreateInstance::<_, IUIAutomation>(
                &CUIAutomation,
                None::<&windows::core::IUnknown>,
                CLSCTX_INPROC_SERVER,
            )
        }
        .ok();
        if automation.is_none() {
            log::warn!("failed to create IUIAutomation instance; will use clipboard fallback");
        }
        Self { automation }
    }

    /// Reads selected text from the focused UI Automation element ONLY.
    /// Deliberately never walks the UIA tree (walking Chromium/Electron trees can push them
    /// into a heavier accessibility mode).
    pub fn read_focused_selection(&self) -> Option<String> {
        let uia = self.automation.as_ref()?;

        // SAFETY: Standard UIA COM calls on the focused element only.
        unsafe {
            let element = uia.GetFocusedElement().ok()?;

            // Try TextPattern2 first, then fall back to TextPattern.
            let ranges = element
                .GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
                .and_then(|p| p.GetSelection())
                .or_else(|_| {
                    element
                        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                        .and_then(|p| p.GetSelection())
                })
                .ok()?;

            let count = ranges.Length().ok()?;
            if count <= 0 {
                return None;
            }

            let mut collected = String::new();
            for i in 0..count {
                if let Ok(range) = ranges.GetElement(i)
                    && let Ok(bstr) = range.GetText(-1)
                {
                    collected.push_str(&bstr.to_string());
                }
            }

            if collected.is_empty() {
                None
            } else {
                Some(collected)
            }
        }
    }
}
