use drmod_hudhook::*;

mod support;

/// Entry point created by the `drmod-hudhook` library.
///
/// # Safety
///
/// haha
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    hmodule: ::drmod_hudhook::windows::Win32::Foundation::HINSTANCE,
    reason: u32,
    _: *mut ::std::ffi::c_void,
) {
    if reason == ::drmod_hudhook::windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH {
        support::setup_tracing();
        ::drmod_hudhook::tracing::trace!("DllMain()");
        let hmodule_raw = hmodule.0 as usize;
        ::std::thread::spawn(move || {
            let hmodule = ::drmod_hudhook::windows::Win32::Foundation::HINSTANCE(hmodule_raw as _);
            if let Err(e) = ::drmod_hudhook::Hudhook::builder()
                .with::<hooks::dx9::ImguiDx9Hooks>(support::HookExample::new())
                .with_hmodule(hmodule)
                .build()
                .apply()
            {
                ::drmod_hudhook::tracing::error!("Couldn't apply hooks: {e:?}");
                ::drmod_hudhook::eject();
            }
        });
    }
}
