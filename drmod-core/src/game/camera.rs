//! Сущность камеры (cCameraGame). Инкапсулирует статический указатель
//! (`base + 0x17EA1D0`) и методы чтения состояния камеры. Наружу отдаёт
//! только `read_*`/`view_proj`/`pos` — сырой указатель наружу не светится.

use std::ptr::NonNull;

use crate::tas::types;

pub(crate) struct Camera {
    /// Статический адрес cCameraGame::Instance: `base + 0x17EA1D0` (SDK).
    camera_ptr_addr: Option<NonNull<u8>>,
}

impl Camera {
    /// Вычисляет статический адрес камеры относительно базового адреса
    /// приложения. `base_addr == 0` — модуль не найден, сущность неактивна.
    pub(crate) fn new(base_addr: usize) -> Self {
        let camera_ptr_addr = if base_addr == 0 {
            None
        } else {
            // base + 0x17EA1D0 — статический адрес cCameraGame::Instance (SDK)
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) })
        };
        Self { camera_ptr_addr }
    }

    /// Читает состояние камеры: позиция (+0x1B0), look-at (+0x1C0), крен
    /// (+0x1F0) и view-proj матрица (+0x200). Смещения — из
    /// `Hw::cCameraBase`/`cCameraViewProj` (ref/mgr-plugin-sdk).
    pub(crate) fn read_camera_state(&self) -> Option<types::CameraState> {
        let addr = self.camera_ptr_addr?.as_ptr();
        Some(unsafe {
            types::CameraState {
                pos: [
                    *(addr.add(0x1B0) as *const f32),
                    *(addr.add(0x1B4) as *const f32),
                    *(addr.add(0x1B8) as *const f32),
                ],
                look_at: [
                    *(addr.add(0x1C0) as *const f32),
                    *(addr.add(0x1C4) as *const f32),
                    *(addr.add(0x1C8) as *const f32),
                ],
                roll: *(addr.add(0x1F0) as *const f32),
                view_proj: *(addr.add(0x200) as *const [f32; 16]),
            }
        })
    }

    /// View-projection матрица камеры (+0x200) — для 3D-рендера.
    pub(crate) fn view_proj(&self) -> Option<[f32; 16]> {
        let addr = self.camera_ptr_addr?.as_ptr();
        Some(unsafe { *(addr.add(0x200) as *const [f32; 16]) })
    }

    /// Позиция камеры (+0x1B0) — для world-to-screen проекции.
    pub(crate) fn pos(&self) -> Option<[f32; 3]> {
        let addr = self.camera_ptr_addr?.as_ptr();
        Some(unsafe {
            [
                *(addr.add(0x1B0) as *const f32),
                *(addr.add(0x1B4) as *const f32),
                *(addr.add(0x1B8) as *const f32),
            ]
        })
    }
}
