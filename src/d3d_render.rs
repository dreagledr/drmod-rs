//! 3D rendering helpers — cylinder primitive via D3D9 fixed-function pipeline.

use hudhook::IDirect3DDevice9;
use windows::Win32::Graphics::Direct3D9::*;
use windows_numerics::Matrix4x4;

// D3DTS_WORLD is missing from the windows crate; define manually.
const D3DTS_WORLD: D3DTRANSFORMSTATETYPE = D3DTRANSFORMSTATETYPE(256);

const IDENTITY_MATRIX: Matrix4x4 = Matrix4x4 {
    M11: 1.0, M12: 0.0, M13: 0.0, M14: 0.0,
    M21: 0.0, M22: 1.0, M23: 0.0, M24: 0.0,
    M31: 0.0, M32: 0.0, M33: 1.0, M34: 0.0,
    M41: 0.0, M42: 0.0, M43: 0.0, M44: 1.0,
};

/// Vertex format: position (3×f32) + diffuse colour (D3DCOLOR AABBGGRR).
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    color: u32,
}

/// Pre-generated cylinder geometry: unit radius, height=1, centred vertically.
pub struct CylinderRenderer {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

impl CylinderRenderer {
    /// Generate a cylinder (radius=1, height=1) with the given colour and
    /// number of slices around the circumference.
    pub fn new(slices: u32, color: u32) -> Self {
        let mut verts = Vec::new();
        let mut idx = Vec::new();

        let top_y = 1.0;
        let bot_y = 0.0;

        // Vertices: top circle (0..slices), bottom circle (slices..2*slices),
        // top centre (2*slices), bottom centre (2*slices+1)
        for i in 0..slices {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / slices as f32;
            let x = angle.cos();
            let z = angle.sin();
            verts.push(Vertex { pos: [x, top_y, z], color });
        }
        for i in 0..slices {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / slices as f32;
            let x = angle.cos();
            let z = angle.sin();
            verts.push(Vertex { pos: [x, bot_y, z], color });
        }
        let top_center = verts.len() as u16;
        verts.push(Vertex { pos: [0.0, top_y, 0.0], color });
        let bot_center = verts.len() as u16;
        verts.push(Vertex { pos: [0.0, bot_y, 0.0], color });

        // Side faces (quads → 2 triangles each)
        for i in 0..slices {
            let t0 = i as u16;
            let t1 = ((i + 1) % slices) as u16;
            let b0 = (slices + i) as u16;
            let b1 = (slices + (i + 1) % slices) as u16;

            idx.extend_from_slice(&[t0, b0, t1]);
            idx.extend_from_slice(&[t1, b0, b1]);
        }

        // Top cap (triangle fan — CCW from above)
        for i in 0..slices {
            let t0 = i as u16;
            let t1 = ((i + 1) % slices) as u16;
            idx.extend_from_slice(&[top_center, t0, t1]);
        }

        // Bottom cap (triangle fan — CCW from below)
        for i in 0..slices {
            let b0 = (slices + i) as u16;
            let b1 = (slices + (i + 1) % slices) as u16;
            idx.extend_from_slice(&[bot_center, b1, b0]);
        }

        Self { vertices: verts, indices: idx }
    }

    // ── Low-level draw ──────────────────────────────────────────────

    unsafe fn draw_raw(&self, device: &IDirect3DDevice9) {
        let stride = std::mem::size_of::<Vertex>() as u32;
        unsafe {
            device
                .DrawIndexedPrimitiveUP(
                    D3DPT_TRIANGLELIST,
                    0,
                    self.vertices.len() as u32,
                    self.indices.len() as u32 / 3,
                    self.indices.as_ptr().cast(),
                    D3DFMT_INDEX16,
                    self.vertices.as_ptr().cast(),
                    stride,
                )
                .ok();
        }
    }

    // ── Public API ───────────────────────────────────────────────────

    /// Render a single opaque cylinder at `base_pos` (cylinder bottom),
    /// scaled by `(radius, height, radius)`, using the game camera's
    /// combined view×proj matrix.
    pub fn render(
        &self,
        device: &IDirect3DDevice9,
        base_pos: (f32, f32, f32),
        radius: f32,
        height: f32,
        color: u32,
        view_proj: &[f32; 16],
    ) {
        unsafe {
            // ── Save state ──────────────────────────────────────────
            let mut saved_fvf: u32 = 0;
            let _ = device.GetFVF(&mut saved_fvf);

            macro_rules! get_rs {
                ($rs:ident, $def:expr) => {{
                    let mut v: u32 = 0;
                    device.GetRenderState($rs, &mut v).ok();
                    if v == 0 { $def } else { v }
                }};
            }

            let saved_zenable    = get_rs!(D3DRS_ZENABLE, 1);
            let saved_zwrite     = get_rs!(D3DRS_ZWRITEENABLE, 1);
            let saved_zfunc      = get_rs!(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32);
            let saved_lighting   = get_rs!(D3DRS_LIGHTING, 1);
            let saved_cull       = get_rs!(D3DRS_CULLMODE, D3DCULL_CCW.0 as u32);
            let saved_alphablend = get_rs!(D3DRS_ALPHABLENDENABLE, 0);
            let saved_alphatest  = get_rs!(D3DRS_ALPHATESTENABLE, 0);
            let saved_sepblend   = get_rs!(D3DRS_SEPARATEALPHABLENDENABLE, 0);
            let saved_covertex   = get_rs!(D3DRS_COLORVERTEX, 1);
            let saved_fill       = get_rs!(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32);
            let saved_srcblend   = get_rs!(D3DRS_SRCBLEND, D3DBLEND_ONE.0 as u32);
            let saved_dstblend   = get_rs!(D3DRS_DESTBLEND, D3DBLEND_ZERO.0 as u32);
            let saved_ambient    = get_rs!(D3DRS_AMBIENT, 0);
            let saved_texfactor  = get_rs!(D3DRS_TEXTUREFACTOR, 0xFFFFFFFF);

            macro_rules! get_tss {
                ($tss:ident, $def:expr) => {{
                    let mut v: u32 = 0;
                    device.GetTextureStageState(0, $tss, &mut v).ok();
                    if v == 0 { $def } else { v }
                }};
            }
            let saved_colorop = get_tss!(D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32);
            let saved_alphaop = get_tss!(D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32);

            // ── Set our state ───────────────────────────────────────
            device.SetVertexShader(None).ok();
            device.SetPixelShader(None).ok();
            device.SetVertexDeclaration(None).ok();
            device.SetFVF(D3DFVF_XYZ | D3DFVF_DIFFUSE).ok();

            // VIEW: combined view×proj from game camera
            let vp = view_proj_to_matrix4x4(view_proj);
            device.SetTransform(D3DTS_VIEW, &vp).ok();
            device.SetTransform(D3DTS_PROJECTION, &IDENTITY_MATRIX).ok();

            // WORLD: scale + translate to position
            let (tx, ty, tz) = base_pos;
            let world = Matrix4x4 {
                M11: radius, M12: 0.0,    M13: 0.0,    M14: 0.0,
                M21: 0.0,    M22: height, M23: 0.0,    M24: 0.0,
                M31: 0.0,    M32: 0.0,    M33: radius, M34: 0.0,
                M41: tx,     M42: ty,     M43: tz,     M44: 1.0,
            };
            device.SetTransform(D3DTS_WORLD, &world).ok();

            // Render states — semi-transparent, no lighting
            device.SetRenderState(D3DRS_ZENABLE, D3DZB_TRUE.0 as u32).ok();
            device.SetRenderState(D3DRS_ZWRITEENABLE, 0).ok();
            device.SetRenderState(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32).ok();
            device.SetRenderState(D3DRS_LIGHTING, 0).ok();
            device.SetRenderState(D3DRS_CULLMODE, D3DCULL_NONE.0 as u32).ok();
            device.SetRenderState(D3DRS_ALPHABLENDENABLE, 1).ok();
            device.SetRenderState(D3DRS_SRCBLEND, D3DBLEND_SRCALPHA.0 as u32).ok();
            device.SetRenderState(D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32).ok();
            device.SetRenderState(D3DRS_ALPHATESTENABLE, 0).ok();
            device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, 0).ok();
            device.SetRenderState(D3DRS_COLORVERTEX, 1).ok();
            device.SetRenderState(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32).ok();
            device.SetRenderState(D3DRS_AMBIENT, 0x00FFFFFF).ok();

            // Use MODULATE: vertex diffuse (white) × texture factor (colour+alpha)
            device.SetRenderState(D3DRS_TEXTUREFACTOR, color).ok();
            device.SetTextureStageState(0, D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32).ok();
            device.SetTextureStageState(0, D3DTSS_COLORARG1, D3DTA_DIFFUSE).ok();
            device.SetTextureStageState(0, D3DTSS_COLORARG2, D3DTA_TFACTOR).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAARG1, D3DTA_DIFFUSE).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAARG2, D3DTA_TFACTOR).ok();

            // ── Draw ────────────────────────────────────────────────
            self.draw_raw(device);

            // ── Restore state ───────────────────────────────────────
            device.SetFVF(saved_fvf).ok();
            device.SetRenderState(D3DRS_ZENABLE, saved_zenable).ok();
            device.SetRenderState(D3DRS_ZWRITEENABLE, saved_zwrite).ok();
            device.SetRenderState(D3DRS_ZFUNC, saved_zfunc).ok();
            device.SetRenderState(D3DRS_LIGHTING, saved_lighting).ok();
            device.SetRenderState(D3DRS_CULLMODE, saved_cull).ok();
            device.SetRenderState(D3DRS_ALPHABLENDENABLE, saved_alphablend).ok();
            device.SetRenderState(D3DRS_SRCBLEND, saved_srcblend).ok();
            device.SetRenderState(D3DRS_DESTBLEND, saved_dstblend).ok();
            device.SetRenderState(D3DRS_ALPHATESTENABLE, saved_alphatest).ok();
            device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, saved_sepblend).ok();
            device.SetRenderState(D3DRS_COLORVERTEX, saved_covertex).ok();
            device.SetRenderState(D3DRS_FILLMODE, saved_fill).ok();
            device.SetRenderState(D3DRS_AMBIENT, saved_ambient).ok();
            device.SetRenderState(D3DRS_TEXTUREFACTOR, saved_texfactor).ok();
            device.SetTextureStageState(0, D3DTSS_COLOROP, saved_colorop).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAOP, saved_alphaop).ok();
        }
    }

    /// Render an oriented cylinder from `start` to `end` with the given `radius`.
    pub fn render_capsule(
        &self,
        device: &IDirect3DDevice9,
        start: (f32, f32, f32),
        end: (f32, f32, f32),
        radius: f32,
        color: u32,
        view_proj: &[f32; 16],
    ) {
        let (sx, sy, sz) = start;
        let (ex, ey, ez) = end;
        let dx = ex - sx;
        let dy = ey - sy;
        let dz = ez - sz;
        let length = (dx * dx + dy * dy + dz * dz).sqrt();
        if length < 0.001 {
            return;
        }

        let nx = dx / length;
        let ny = dy / length;
        let nz = dz / length;

        let (ax, ay, az) = if nx.abs() < 0.99 {
            let len = (nz * nz + nx * nx).sqrt();
            (nz / len, 0.0, -nx / len)
        } else {
            let len = (nz * nz + ny * ny).sqrt();
            (0.0, nz / len, -ny / len)
        };
        let zx = ny * az - nz * ay;
        let zy = nz * ax - nx * az;
        let zz = nx * ay - ny * ax;

        let world = Matrix4x4 {
            M11: radius * ax, M12: radius * ay, M13: radius * az, M14: 0.0,
            M21: dx,          M22: dy,          M23: dz,          M24: 0.0,
            M31: radius * zx, M32: radius * zy, M33: radius * zz, M34: 0.0,
            M41: sx,          M42: sy,          M43: sz,          M44: 1.0,
        };

        unsafe {
            let mut saved_fvf: u32 = 0;
            let _ = device.GetFVF(&mut saved_fvf);
            macro_rules! get_rs {
                ($rs:ident, $def:expr) => {{
                    let mut v: u32 = 0;
                    device.GetRenderState($rs, &mut v).ok();
                    if v == 0 { $def } else { v }
                }};
            }
            let saved_zenable    = get_rs!(D3DRS_ZENABLE, 1);
            let saved_zwrite     = get_rs!(D3DRS_ZWRITEENABLE, 1);
            let saved_zfunc      = get_rs!(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32);
            let saved_lighting   = get_rs!(D3DRS_LIGHTING, 1);
            let saved_cull       = get_rs!(D3DRS_CULLMODE, D3DCULL_CCW.0 as u32);
            let saved_alphablend = get_rs!(D3DRS_ALPHABLENDENABLE, 0);
            let saved_alphatest  = get_rs!(D3DRS_ALPHATESTENABLE, 0);
            let saved_sepblend   = get_rs!(D3DRS_SEPARATEALPHABLENDENABLE, 0);
            let saved_covertex   = get_rs!(D3DRS_COLORVERTEX, 1);
            let saved_fill       = get_rs!(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32);
            let saved_srcblend   = get_rs!(D3DRS_SRCBLEND, D3DBLEND_ONE.0 as u32);
            let saved_dstblend   = get_rs!(D3DRS_DESTBLEND, D3DBLEND_ZERO.0 as u32);
            let saved_ambient    = get_rs!(D3DRS_AMBIENT, 0);
            let saved_texfactor  = get_rs!(D3DRS_TEXTUREFACTOR, 0xFFFFFFFF);
            macro_rules! get_tss {
                ($tss:ident, $def:expr) => {{
                    let mut v: u32 = 0;
                    device.GetTextureStageState(0, $tss, &mut v).ok();
                    if v == 0 { $def } else { v }
                }};
            }
            let saved_colorop = get_tss!(D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32);
            let saved_alphaop = get_tss!(D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32);

            device.SetVertexShader(None).ok();
            device.SetPixelShader(None).ok();
            device.SetVertexDeclaration(None).ok();
            device.SetFVF(D3DFVF_XYZ | D3DFVF_DIFFUSE).ok();
            let vp = view_proj_to_matrix4x4(view_proj);
            device.SetTransform(D3DTS_VIEW, &vp).ok();
            device.SetTransform(D3DTS_PROJECTION, &IDENTITY_MATRIX).ok();
            device.SetTransform(D3DTS_WORLD, &world).ok();

            device.SetRenderState(D3DRS_ZENABLE, D3DZB_TRUE.0 as u32).ok();
            device.SetRenderState(D3DRS_ZWRITEENABLE, 0).ok();
            device.SetRenderState(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32).ok();
            device.SetRenderState(D3DRS_LIGHTING, 0).ok();
            device.SetRenderState(D3DRS_CULLMODE, D3DCULL_NONE.0 as u32).ok();
            device.SetRenderState(D3DRS_ALPHABLENDENABLE, 1).ok();
            device.SetRenderState(D3DRS_SRCBLEND, D3DBLEND_SRCALPHA.0 as u32).ok();
            device.SetRenderState(D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32).ok();
            device.SetRenderState(D3DRS_ALPHATESTENABLE, 0).ok();
            device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, 0).ok();
            device.SetRenderState(D3DRS_COLORVERTEX, 1).ok();
            device.SetRenderState(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32).ok();
            device.SetRenderState(D3DRS_AMBIENT, 0x00FFFFFF).ok();
            device.SetRenderState(D3DRS_TEXTUREFACTOR, color).ok();
            device.SetTextureStageState(0, D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32).ok();
            device.SetTextureStageState(0, D3DTSS_COLORARG1, D3DTA_DIFFUSE).ok();
            device.SetTextureStageState(0, D3DTSS_COLORARG2, D3DTA_TFACTOR).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAARG1, D3DTA_DIFFUSE).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAARG2, D3DTA_TFACTOR).ok();

            self.draw_raw(device);

            device.SetFVF(saved_fvf).ok();
            device.SetRenderState(D3DRS_ZENABLE, saved_zenable).ok();
            device.SetRenderState(D3DRS_ZWRITEENABLE, saved_zwrite).ok();
            device.SetRenderState(D3DRS_ZFUNC, saved_zfunc).ok();
            device.SetRenderState(D3DRS_LIGHTING, saved_lighting).ok();
            device.SetRenderState(D3DRS_CULLMODE, saved_cull).ok();
            device.SetRenderState(D3DRS_ALPHABLENDENABLE, saved_alphablend).ok();
            device.SetRenderState(D3DRS_SRCBLEND, saved_srcblend).ok();
            device.SetRenderState(D3DRS_DESTBLEND, saved_dstblend).ok();
            device.SetRenderState(D3DRS_ALPHATESTENABLE, saved_alphatest).ok();
            device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, saved_sepblend).ok();
            device.SetRenderState(D3DRS_COLORVERTEX, saved_covertex).ok();
            device.SetRenderState(D3DRS_FILLMODE, saved_fill).ok();
            device.SetRenderState(D3DRS_AMBIENT, saved_ambient).ok();
            device.SetRenderState(D3DRS_TEXTUREFACTOR, saved_texfactor).ok();
            device.SetTextureStageState(0, D3DTSS_COLOROP, saved_colorop).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAOP, saved_alphaop).ok();
        }
    }
}

// ── Sphere renderer ─────────────────────────────────────────────────

pub struct SphereRenderer {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

impl SphereRenderer {
    pub fn new(slices: u32, stacks: u32, color: u32) -> Self {
        let mut verts = Vec::new();
        let mut idx = Vec::new();
        verts.push(Vertex { pos: [0.0, 1.0, 0.0], color });
        verts.push(Vertex { pos: [0.0, -1.0, 0.0], color });
        for j in 1..stacks {
            let phi = std::f32::consts::PI * j as f32 / stacks as f32;
            let y = phi.cos();
            let r = phi.sin();
            for i in 0..slices {
                let theta = 2.0 * std::f32::consts::PI * i as f32 / slices as f32;
                verts.push(Vertex { pos: [r * theta.cos(), y, r * theta.sin()], color });
            }
        }
        for i in 0..slices {
            let next = (i + 1) % slices;
            idx.extend_from_slice(&[0, (2 + i) as u16, (2 + next) as u16]);
        }
        for j in 0..stacks - 2 {
            let row = 2 + j * slices;
            let next_row = row + slices;
            for i in 0..slices {
                let next = (i + 1) % slices;
                let a = (row + i) as u16;
                let b = (row + next) as u16;
                let c = (next_row + i) as u16;
                let d = (next_row + next) as u16;
                idx.extend_from_slice(&[a, b, d]);
                idx.extend_from_slice(&[a, d, c]);
            }
        }
        let bottom_row = 2 + (stacks - 2) * slices;
        for i in 0..slices {
            let next = (i + 1) % slices;
            idx.extend_from_slice(&[1, (bottom_row + next) as u16, (bottom_row + i) as u16]);
        }
        Self { vertices: verts, indices: idx }
    }

    unsafe fn draw_raw(&self, device: &IDirect3DDevice9) {
        let stride = std::mem::size_of::<Vertex>() as u32;
        unsafe {
            device.DrawIndexedPrimitiveUP(
                D3DPT_TRIANGLELIST,
                0,
                self.vertices.len() as u32,
                self.indices.len() as u32 / 3,
                self.indices.as_ptr().cast(),
                D3DFMT_INDEX16,
                self.vertices.as_ptr().cast(),
                stride,
            ).ok();
        }
    }

    pub fn render(
        &self,
        device: &IDirect3DDevice9,
        center: (f32, f32, f32),
        radius: f32,
        color: u32,
        view_proj: &[f32; 16],
    ) {
        let (cx, cy, cz) = center;
        let world = Matrix4x4 {
            M11: radius, M12: 0.0, M13: 0.0, M14: 0.0,
            M21: 0.0, M22: radius, M23: 0.0, M24: 0.0,
            M31: 0.0, M32: 0.0, M33: radius, M34: 0.0,
            M41: cx, M42: cy, M43: cz, M44: 1.0,
        };
        unsafe {
            let mut saved_fvf: u32 = 0;
            let _ = device.GetFVF(&mut saved_fvf);
            macro_rules! get_rs {
                ($rs:ident, $def:expr) => {{
                    let mut v: u32 = 0;
                    device.GetRenderState($rs, &mut v).ok();
                    if v == 0 { $def } else { v }
                }};
            }
            let saved_zenable    = get_rs!(D3DRS_ZENABLE, 1);
            let saved_zwrite     = get_rs!(D3DRS_ZWRITEENABLE, 1);
            let saved_zfunc      = get_rs!(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32);
            let saved_lighting   = get_rs!(D3DRS_LIGHTING, 1);
            let saved_cull       = get_rs!(D3DRS_CULLMODE, D3DCULL_CCW.0 as u32);
            let saved_alphablend = get_rs!(D3DRS_ALPHABLENDENABLE, 0);
            let saved_alphatest  = get_rs!(D3DRS_ALPHATESTENABLE, 0);
            let saved_sepblend   = get_rs!(D3DRS_SEPARATEALPHABLENDENABLE, 0);
            let saved_covertex   = get_rs!(D3DRS_COLORVERTEX, 1);
            let saved_fill       = get_rs!(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32);
            let saved_srcblend   = get_rs!(D3DRS_SRCBLEND, D3DBLEND_ONE.0 as u32);
            let saved_dstblend   = get_rs!(D3DRS_DESTBLEND, D3DBLEND_ZERO.0 as u32);
            let saved_ambient    = get_rs!(D3DRS_AMBIENT, 0);
            let saved_texfactor  = get_rs!(D3DRS_TEXTUREFACTOR, 0xFFFFFFFF);
            macro_rules! get_tss {
                ($tss:ident, $def:expr) => {{
                    let mut v: u32 = 0;
                    device.GetTextureStageState(0, $tss, &mut v).ok();
                    if v == 0 { $def } else { v }
                }};
            }
            let saved_colorop = get_tss!(D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32);
            let saved_alphaop = get_tss!(D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32);

            device.SetVertexShader(None).ok();
            device.SetPixelShader(None).ok();
            device.SetVertexDeclaration(None).ok();
            device.SetFVF(D3DFVF_XYZ | D3DFVF_DIFFUSE).ok();
            let vp = view_proj_to_matrix4x4(view_proj);
            device.SetTransform(D3DTS_VIEW, &vp).ok();
            device.SetTransform(D3DTS_PROJECTION, &IDENTITY_MATRIX).ok();
            device.SetTransform(D3DTS_WORLD, &world).ok();

            device.SetRenderState(D3DRS_ZENABLE, D3DZB_TRUE.0 as u32).ok();
            device.SetRenderState(D3DRS_ZWRITEENABLE, 0).ok();
            device.SetRenderState(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32).ok();
            device.SetRenderState(D3DRS_LIGHTING, 0).ok();
            device.SetRenderState(D3DRS_CULLMODE, D3DCULL_NONE.0 as u32).ok();
            device.SetRenderState(D3DRS_ALPHABLENDENABLE, 1).ok();
            device.SetRenderState(D3DRS_SRCBLEND, D3DBLEND_SRCALPHA.0 as u32).ok();
            device.SetRenderState(D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32).ok();
            device.SetRenderState(D3DRS_ALPHATESTENABLE, 0).ok();
            device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, 0).ok();
            device.SetRenderState(D3DRS_COLORVERTEX, 1).ok();
            device.SetRenderState(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32).ok();
            device.SetRenderState(D3DRS_AMBIENT, 0x00FFFFFF).ok();
            device.SetRenderState(D3DRS_TEXTUREFACTOR, color).ok();
            device.SetTextureStageState(0, D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32).ok();
            device.SetTextureStageState(0, D3DTSS_COLORARG1, D3DTA_DIFFUSE).ok();
            device.SetTextureStageState(0, D3DTSS_COLORARG2, D3DTA_TFACTOR).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAARG1, D3DTA_DIFFUSE).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAARG2, D3DTA_TFACTOR).ok();

            self.draw_raw(device);

            device.SetFVF(saved_fvf).ok();
            device.SetRenderState(D3DRS_ZENABLE, saved_zenable).ok();
            device.SetRenderState(D3DRS_ZWRITEENABLE, saved_zwrite).ok();
            device.SetRenderState(D3DRS_ZFUNC, saved_zfunc).ok();
            device.SetRenderState(D3DRS_LIGHTING, saved_lighting).ok();
            device.SetRenderState(D3DRS_CULLMODE, saved_cull).ok();
            device.SetRenderState(D3DRS_ALPHABLENDENABLE, saved_alphablend).ok();
            device.SetRenderState(D3DRS_SRCBLEND, saved_srcblend).ok();
            device.SetRenderState(D3DRS_DESTBLEND, saved_dstblend).ok();
            device.SetRenderState(D3DRS_ALPHATESTENABLE, saved_alphatest).ok();
            device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, saved_sepblend).ok();
            device.SetRenderState(D3DRS_COLORVERTEX, saved_covertex).ok();
            device.SetRenderState(D3DRS_FILLMODE, saved_fill).ok();
            device.SetRenderState(D3DRS_AMBIENT, saved_ambient).ok();
            device.SetRenderState(D3DRS_TEXTUREFACTOR, saved_texfactor).ok();
            device.SetTextureStageState(0, D3DTSS_COLOROP, saved_colorop).ok();
            device.SetTextureStageState(0, D3DTSS_ALPHAOP, saved_alphaop).ok();
        }
    }
}

// ── 3D line rendering ───────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
struct LineVertex {
    pos: [f32; 3],
}

pub unsafe fn draw_lines_3d(
    device: &IDirect3DDevice9,
    segments: &[(f32, f32, f32, f32, f32, f32)],
    color: u32,
    view_proj: &[f32; 16],
) {
    if segments.is_empty() {
        return;
    }
    unsafe {
        let mut saved_fvf: u32 = 0;
        let _ = device.GetFVF(&mut saved_fvf);
        macro_rules! get_rs {
            ($rs:ident, $def:expr) => {{
                let mut v: u32 = 0;
                device.GetRenderState($rs, &mut v).ok();
                if v == 0 { $def } else { v }
            }};
        }
        let saved_zenable    = get_rs!(D3DRS_ZENABLE, 1);
        let saved_zwrite     = get_rs!(D3DRS_ZWRITEENABLE, 1);
        let saved_zfunc      = get_rs!(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32);
        let saved_lighting   = get_rs!(D3DRS_LIGHTING, 1);
        let saved_cull       = get_rs!(D3DRS_CULLMODE, D3DCULL_CCW.0 as u32);
        let saved_alphablend = get_rs!(D3DRS_ALPHABLENDENABLE, 0);
        let saved_alphatest  = get_rs!(D3DRS_ALPHATESTENABLE, 0);
        let saved_sepblend   = get_rs!(D3DRS_SEPARATEALPHABLENDENABLE, 0);
        let saved_covertex   = get_rs!(D3DRS_COLORVERTEX, 1);
        let saved_fill       = get_rs!(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32);
        let saved_srcblend   = get_rs!(D3DRS_SRCBLEND, D3DBLEND_ONE.0 as u32);
        let saved_dstblend   = get_rs!(D3DRS_DESTBLEND, D3DBLEND_ZERO.0 as u32);
        let saved_ambient    = get_rs!(D3DRS_AMBIENT, 0);
        let saved_texfactor  = get_rs!(D3DRS_TEXTUREFACTOR, 0xFFFFFFFF);
        macro_rules! get_tss {
            ($tss:ident, $def:expr) => {{
                let mut v: u32 = 0;
                device.GetTextureStageState(0, $tss, &mut v).ok();
                if v == 0 { $def } else { v }
            }};
        }
        let saved_colorop = get_tss!(D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32);
        let saved_alphaop = get_tss!(D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32);

        device.SetVertexShader(None).ok();
        device.SetPixelShader(None).ok();
        device.SetVertexDeclaration(None).ok();
        device.SetFVF(D3DFVF_XYZ).ok();
        let vp = view_proj_to_matrix4x4(view_proj);
        device.SetTransform(D3DTS_VIEW, &vp).ok();
        device.SetTransform(D3DTS_PROJECTION, &IDENTITY_MATRIX).ok();
        device.SetTransform(D3DTS_WORLD, &IDENTITY_MATRIX).ok();

        device.SetRenderState(D3DRS_ZENABLE, D3DZB_TRUE.0 as u32).ok();
        device.SetRenderState(D3DRS_ZWRITEENABLE, 0).ok();
        device.SetRenderState(D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32).ok();
        device.SetRenderState(D3DRS_LIGHTING, 0).ok();
        device.SetRenderState(D3DRS_CULLMODE, D3DCULL_NONE.0 as u32).ok();
        device.SetRenderState(D3DRS_ALPHABLENDENABLE, 1).ok();
        device.SetRenderState(D3DRS_SRCBLEND, D3DBLEND_SRCALPHA.0 as u32).ok();
        device.SetRenderState(D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32).ok();
        device.SetRenderState(D3DRS_ALPHATESTENABLE, 0).ok();
        device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, 0).ok();
        device.SetRenderState(D3DRS_COLORVERTEX, 1).ok();
        device.SetRenderState(D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32).ok();
        device.SetRenderState(D3DRS_AMBIENT, 0x00FFFFFF).ok();
        device.SetRenderState(D3DRS_TEXTUREFACTOR, color).ok();
        device.SetTextureStageState(0, D3DTSS_COLOROP, D3DTOP_SELECTARG1.0 as u32).ok();
        device.SetTextureStageState(0, D3DTSS_COLORARG1, D3DTA_TFACTOR).ok();
        device.SetTextureStageState(0, D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32).ok();
        device.SetTextureStageState(0, D3DTSS_ALPHAARG1, D3DTA_TFACTOR).ok();

        let verts: Vec<LineVertex> = segments.iter().flat_map(|&(x1, y1, z1, x2, y2, z2)| {
            [LineVertex { pos: [x1, y1, z1] }, LineVertex { pos: [x2, y2, z2] }]
        }).collect();
        let stride = std::mem::size_of::<LineVertex>() as u32;
        device.DrawPrimitiveUP(D3DPT_LINELIST, (verts.len() / 2) as u32, verts.as_ptr().cast(), stride).ok();

        device.SetFVF(saved_fvf).ok();
        device.SetRenderState(D3DRS_ZENABLE, saved_zenable).ok();
        device.SetRenderState(D3DRS_ZWRITEENABLE, saved_zwrite).ok();
        device.SetRenderState(D3DRS_ZFUNC, saved_zfunc).ok();
        device.SetRenderState(D3DRS_LIGHTING, saved_lighting).ok();
        device.SetRenderState(D3DRS_CULLMODE, saved_cull).ok();
        device.SetRenderState(D3DRS_ALPHABLENDENABLE, saved_alphablend).ok();
        device.SetRenderState(D3DRS_SRCBLEND, saved_srcblend).ok();
        device.SetRenderState(D3DRS_DESTBLEND, saved_dstblend).ok();
        device.SetRenderState(D3DRS_ALPHATESTENABLE, saved_alphatest).ok();
        device.SetRenderState(D3DRS_SEPARATEALPHABLENDENABLE, saved_sepblend).ok();
        device.SetRenderState(D3DRS_COLORVERTEX, saved_covertex).ok();
        device.SetRenderState(D3DRS_FILLMODE, saved_fill).ok();
        device.SetRenderState(D3DRS_AMBIENT, saved_ambient).ok();
        device.SetRenderState(D3DRS_TEXTUREFACTOR, saved_texfactor).ok();
        device.SetTextureStageState(0, D3DTSS_COLOROP, saved_colorop).ok();
        device.SetTextureStageState(0, D3DTSS_ALPHAOP, saved_alphaop).ok();
    }
}

/// Convert a row-major `[f32; 16]` to `Matrix4x4`.
fn view_proj_to_matrix4x4(m: &[f32; 16]) -> Matrix4x4 {
    Matrix4x4 {
        M11: m[0],  M12: m[1],  M13: m[2],  M14: m[3],
        M21: m[4],  M22: m[5],  M23: m[6],  M24: m[7],
        M31: m[8],  M32: m[9],  M33: m[10], M34: m[11],
        M41: m[12], M42: m[13], M43: m[14], M44: m[15],
    }
}
