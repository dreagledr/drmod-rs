//! Панели редактора. Каждая — функция рендера, а не отдельный `Component`:
//! состояние одно (`Editor`), а панели только показывают его и шлют сообщения.

pub(super) mod properties;
pub(super) mod script_list;
pub(super) mod text_editor;
pub(super) mod timeline;
