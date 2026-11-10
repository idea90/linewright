//! Keeps the triangles of a picture of the drawing on the graphics card, and draws them from there.
//!
//! egui takes every shape afresh on every frame and copies all its triangles to the card again. For a picture of
//! millions of triangles that does not change from one frame to the next that is nearly all a frame costs, so
//! such a picture is sent once and drawn from the card's own copy, moved and scaled there while the view is
//! panned or zoomed. `Gpu::draw` runs inside an egui paint callback, where the OpenGL context is current.

use std::collections::HashMap;

use eframe::egui::{PaintCallbackInfo, Vec2, epaint::Vertex};
use eframe::egui_glow::ShaderVersion;
use eframe::glow::{self, HasContext as _};

/// Where a picture's points go on screen now: each is scaled, then moved. A picture made for the view on screen
/// stays where it is; one made for an earlier view is put where the view has gone since.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub scale: f32,
    pub offset: Vec2,
}

impl Placement {
    pub const AS_MADE: Placement = Placement { scale: 1.0, offset: Vec2::ZERO };
}

/// Frames a picture may go undrawn before the card's copy of it is let go.
const FRAMES_KEPT: u64 = 2;

const VERTEX_SHADER: &str = r#"
#if NEW_SHADER_INTERFACE
    #define I in
    #define O out
#else
    #define I attribute
    #define O varying
#endif
#ifdef GL_ES
    precision mediump float;
#endif
uniform vec2 u_size;      // of the area drawn in, in points
uniform vec3 u_placement; // offset x, offset y, scale
I vec2 a_pos;
I vec4 a_color;           // premultiplied, in gamma space, as egui's own
O vec4 v_color;
void main() {
    vec2 at = a_pos * u_placement.z + u_placement.xy;
    gl_Position = vec4(2.0 * at.x / u_size.x - 1.0, 1.0 - 2.0 * at.y / u_size.y, 0.0, 1.0);
    v_color = a_color;
}
"#;

const FRAGMENT_SHADER: &str = r#"
#ifdef GL_ES
    precision mediump float;
#endif
#if NEW_SHADER_INTERFACE
    in vec4 v_color;
    out vec4 f_color;
    #define gl_FragColor f_color
#else
    varying vec4 v_color;
#endif
void main() {
    gl_FragColor = v_color;
}
"#;

struct Program {
    program: glow::Program,
    size: glow::UniformLocation,
    placement: glow::UniformLocation,
    position: u32,
    color: u32,
}

/// A picture's triangles on the card.
struct Buffers {
    vertex_array: glow::VertexArray,
    vertices: glow::Buffer,
    indices: glow::Buffer,
    last_drawn: u64,
}

/// The pictures the card holds, and the program it draws them with.
#[derive(Default)]
pub struct Gpu {
    program: Option<Program>,
    pictures: HashMap<u64, Buffers>,
    /// The card would not take the program or a picture: the caller draws through egui from then on.
    broken: bool,
}

impl Gpu {
    pub fn broken(&self) -> bool {
        self.broken
    }

    /// Draw triangles of the picture `id` on frame `frame`: `count` corners from the `first`th. The picture's
    /// meshes, which `meshes` gives in the order their corners are counted in, go to the card the first time.
    pub fn draw<'a>(
        &mut self,
        gl: &glow::Context,
        info: &PaintCallbackInfo,
        (id, frame): (u64, u64),
        meshes: impl Fn() -> Vec<&'a eframe::egui::Mesh>,
        (first, count): (u32, u32),
        placement: Placement,
    ) {
        if self.broken || count == 0 {
            return;
        }
        if self.draw_or_fail(gl, info, (id, frame), meshes, (first, count), placement).is_err() {
            self.broken = true;
        }
    }

    fn draw_or_fail<'a>(
        &mut self,
        gl: &glow::Context,
        info: &PaintCallbackInfo,
        (id, frame): (u64, u64),
        meshes: impl Fn() -> Vec<&'a eframe::egui::Mesh>,
        (first, count): (u32, u32),
        placement: Placement,
    ) -> Result<(), String> {
        if self.program.is_none() {
            self.program = Some(make_program(gl)?);
        }
        let Some(program) = &self.program else { return Err("no program".into()) };
        if !self.pictures.contains_key(&id) {
            // Pictures no longer shown make room first.
            let stale: Vec<u64> = self.pictures.iter().filter(|(_, b)| b.last_drawn + FRAMES_KEPT < frame).map(|(id, _)| *id).collect();
            for old in stale {
                if let Some(buffers) = self.pictures.remove(&old) {
                    free(gl, &buffers);
                }
            }
            let buffers = send(gl, program, &meshes())?;
            self.pictures.insert(id, buffers);
        }
        let Some(buffers) = self.pictures.get_mut(&id) else { return Err("no picture".into()) };
        buffers.last_drawn = frame;
        // The callback's viewport is the area drawn in, on whole pixels: points are counted from its corner there.
        let (pixels, per_point) = (info.viewport_in_pixels(), info.pixels_per_point);
        let corner = eframe::egui::vec2(pixels.left_px as f32, pixels.top_px as f32) / per_point;
        let size = eframe::egui::vec2(pixels.width_px as f32, pixels.height_px as f32) / per_point;
        let offset = placement.offset - corner;
        // SAFETY: the OpenGL context is current while egui paints, which is when this runs, and every name used
        // here was made in it by `make_program` and `send`.
        unsafe {
            gl.use_program(Some(program.program));
            gl.uniform_2_f32(Some(&program.size), size.x, size.y);
            gl.uniform_3_f32(Some(&program.placement), offset.x, offset.y, placement.scale);
            gl.bind_vertex_array(Some(buffers.vertex_array));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(buffers.indices));
            gl.draw_elements(glow::TRIANGLES, count as i32, glow::UNSIGNED_INT, first as i32 * size_of::<u32>() as i32);
            gl.bind_vertex_array(None);
            if gl.get_error() != glow::NO_ERROR {
                return Err("drawing failed".into());
            }
        }
        Ok(())
    }
}

fn free(gl: &glow::Context, buffers: &Buffers) {
    // SAFETY: the context is current, and these names were made in it and are used nowhere else.
    unsafe {
        gl.delete_vertex_array(buffers.vertex_array);
        gl.delete_buffer(buffers.vertices);
        gl.delete_buffer(buffers.indices);
    }
}

fn make_program(gl: &glow::Context) -> Result<Program, String> {
    let version = ShaderVersion::get(gl);
    let header =
        format!("{}\n#define NEW_SHADER_INTERFACE {}\n", version.version_declaration(), i32::from(version.is_new_shader_interface()));
    // SAFETY: the context is current; each call is given names made just above in the same context.
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = vec![];
        for (kind, source) in [(glow::VERTEX_SHADER, VERTEX_SHADER), (glow::FRAGMENT_SHADER, FRAGMENT_SHADER)] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, &format!("{header}{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        let uniform = |name: &str| gl.get_uniform_location(program, name).ok_or_else(|| format!("no uniform {name}"));
        let attribute = |name: &str| gl.get_attrib_location(program, name).ok_or_else(|| format!("no attribute {name}"));
        Ok(Program {
            program,
            size: uniform("u_size")?,
            placement: uniform("u_placement")?,
            position: attribute("a_pos")?,
            color: attribute("a_color")?,
        })
    }
}

/// The corners of a mesh as the bytes the card takes them in.
fn vertex_bytes(vertices: &[Vertex]) -> &[u8] {
    // SAFETY: `Vertex` is `repr(C)` of floats and bytes with no padding (two points and a color, 20 bytes), so its
    // memory is plain bytes, and the slice covers exactly the vertices borrowed.
    unsafe { std::slice::from_raw_parts(vertices.as_ptr().cast::<u8>(), std::mem::size_of_val(vertices)) }
}

fn index_bytes(indices: &[u32]) -> &[u8] {
    // SAFETY: a `u32` is four plain bytes, and the slice covers exactly the indices borrowed.
    unsafe { std::slice::from_raw_parts(indices.as_ptr().cast::<u8>(), std::mem::size_of_val(indices)) }
}

/// Send a picture's meshes to the card as one run of corners and one of triangles.
fn send(gl: &glow::Context, program: &Program, meshes: &[&eframe::egui::Mesh]) -> Result<Buffers, String> {
    let stride = size_of::<Vertex>() as i32;
    let vertex_count: usize = meshes.iter().map(|m| m.vertices.len()).sum();
    if vertex_count > u32::MAX as usize {
        return Err("too many corners".into());
    }
    // Each mesh counts its corners from zero; in the one run they follow the meshes before.
    let mut indices: Vec<u32> = Vec::with_capacity(meshes.iter().map(|m| m.indices.len()).sum());
    let mut before = 0u32;
    for mesh in meshes {
        indices.extend(mesh.indices.iter().map(|i| i + before));
        before += mesh.vertices.len() as u32;
    }
    // SAFETY: the context is current; the buffers are bound before they are filled, and the attribute offsets lie
    // inside a `Vertex` (position at 0, color at 16).
    unsafe {
        let vertex_array = gl.create_vertex_array()?;
        let vertices = gl.create_buffer()?;
        let index_buffer = gl.create_buffer()?;
        let buffers = Buffers { vertex_array, vertices, indices: index_buffer, last_drawn: 0 };
        gl.bind_vertex_array(Some(vertex_array));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
        gl.buffer_data_size(glow::ARRAY_BUFFER, vertex_count as i32 * stride, glow::STATIC_DRAW);
        let mut at = 0;
        for mesh in meshes {
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, at, vertex_bytes(&mesh.vertices));
            at += mesh.vertices.len() as i32 * stride;
        }
        gl.enable_vertex_attrib_array(program.position);
        gl.vertex_attrib_pointer_f32(program.position, 2, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(program.color);
        gl.vertex_attrib_pointer_f32(program.color, 4, glow::UNSIGNED_BYTE, true, stride, 16);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(index_buffer));
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, index_bytes(&indices), glow::STATIC_DRAW);
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        if gl.get_error() != glow::NO_ERROR {
            free(gl, &buffers);
            return Err("the card would not take the picture".into());
        }
        Ok(buffers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, pos2};

    #[test]
    fn a_vertex_is_laid_out_as_the_card_is_told() {
        assert_eq!(size_of::<Vertex>(), 20);
        let vertex = Vertex { pos: pos2(1.0, 2.0), uv: pos2(0.0, 0.0), color: Color32::from_rgba_premultiplied(10, 20, 30, 40) };
        let bytes = vertex_bytes(std::slice::from_ref(&vertex));
        assert_eq!(bytes[..4], 1.0f32.to_ne_bytes());
        assert_eq!(bytes[4..8], 2.0f32.to_ne_bytes());
        assert_eq!(bytes[16..], [10, 20, 30, 40]);
        assert_eq!(index_bytes(&[1, 2]).len(), 8);
    }
}
