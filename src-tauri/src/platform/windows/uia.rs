use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationTextPattern,
    IUIAutomationTextPattern2, UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::core::Interface;

const MAX_UIA_RANGES: i32 = 64;
const UIA_TIMEOUT_MS: u32 = 1500;

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
        if let Some(ref uia) = automation {
            // Bound cross-process UIA COM calls so a hung provider cannot stall the worker for 20s.
            if let Ok(uia2) = uia.cast::<IUIAutomation2>() {
                unsafe {
                    let _ = uia2.SetConnectionTimeout(UIA_TIMEOUT_MS);
                    let _ = uia2.SetTransactionTimeout(UIA_TIMEOUT_MS);
                }
            }
        } else {
            log::warn!("failed to create IUIAutomation instance; will use clipboard fallback");
        }
        Self { automation }
    }

    /// Reads selected text from the focused UI Automation element ONLY, capped at `max_chars + 1`
    /// characters so an untrusted provider cannot trigger unbounded cross-process allocations.
    /// Deliberately never walks the UIA tree (walking Chromium/Electron trees can push them
    /// into a heavier accessibility mode).
    pub fn read_focused_selection(&self, max_chars: usize) -> Option<String> {
        let uia = self.automation.as_ref()?;
        let cap = max_chars.saturating_add(1);

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

            let bounded_count = count.min(MAX_UIA_RANGES);
            let mut collected = String::new();
            let mut collected_chars: usize = 0;

            for i in 0..bounded_count {
                let remaining = cap.saturating_sub(collected_chars);
                if remaining == 0 {
                    break;
                }
                let max_len = i32::try_from(remaining).unwrap_or(i32::MAX);
                if let Ok(range) = ranges.GetElement(i)
                    && let Ok(bstr) = range.GetText(max_len)
                {
                    let part = bstr.to_string();
                    collected_chars = collected_chars.saturating_add(part.chars().count());
                    collected.push_str(&part);
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
