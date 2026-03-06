struct Uniforms {
    width: u32,
    height: u32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var<storage, read_write> snad_mut: array<u32>;

@group(0) @binding(2)
var<storage, read_write> snad_next: array<u32>;

fn offset_index(x: u32, y: u32, offset_x: i32, offset_y: i32) -> u32 {
    let index = x + uniforms.width * y;
    if i32(x) + offset_x >= i32(uniforms.width)
        || i32(x) + offset_x < 0
        || i32(y) + offset_y >= i32(uniforms.height)
        || i32(y) + offset_y < 0
    {
        return u32(-1i);
    }
    let new_index = index + u32(offset_x) + (uniforms.width * u32(offset_y));
    return new_index;
}

fn offset_index_unchecked(x: u32, y: u32, offset_x: i32, offset_y: i32) -> u32 {
    let index = x + uniforms.width * y;
    let new_index = index + u32(offset_x) + (uniforms.width * u32(offset_y));
    return new_index;
}

struct SimImmediate {
    row: u32,
    flag: u32,
}

var<immediate> sim: SimImmediate;

@compute @workgroup_size(64)
fn simulate(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    if x >= uniforms.width {
        return;
    }
    let swap_lr = i32((y & 1) * 2) - 1;
    let s = swap_lr;
    let curr = offset_index_unchecked(x, y, 0, 0); 
    if snad_mut[curr] == 0 {
        snad_next[curr] = snad_mut[curr];
        return;
    }
    let down = offset_index(x, y, 0, -1);
    if down == u32(-1i) {
        snad_next[curr] = snad_mut[curr];
        return;
    }
    if snad_mut[down] == 0 {
        snad_next[down] = snad_mut[curr];
        snad_next[curr] = 0;
        return;
    }
    let left = offset_index(x, y, -1*s, 0);
    let down_left = offset_index_unchecked(x, y, -1*s, -1);
    if left != u32(-1i) && snad_mut[left] == 0 && snad_mut[down_left] == 0 {
        snad_next[down_left] = snad_mut[curr];
        snad_next[curr] = 0;
        return;
    }
    let right = offset_index(x, y, 1*s, 0);
    if right == u32(-1i) {
        snad_next[curr] = snad_mut[curr];
        return;
    }
    let down_right = offset_index_unchecked(x, y, 1*s, -1);
    let two_right = offset_index(x, y, 2*s, 0);
    if two_right == u32(-1i) {
        if snad_mut[right] == 0 && snad_mut[down_right] == 0 {
            snad_next[down_right] = snad_mut[curr];
            snad_next[curr] = 0;
            return;
        }
        snad_next[curr] = snad_mut[curr];
        return;
    } else {
        let down_right_two = offset_index_unchecked(x, y, 2*s, -1);
        if snad_mut[right] == 0 && snad_mut[down_right] == 0 && (snad_mut[two_right] == 0 || snad_mut[down_right_two] == 0) {
            snad_next[down_right] = snad_mut[curr];
            snad_next[curr] = 0;
            return;
        }
        snad_next[curr] = snad_mut[curr];
        return;
    }
}

struct InputImmediate {
    coord: vec2f,
    size: f32,
}

var<immediate> imm: InputImmediate;

@compute @workgroup_size(64)
fn input(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x > uniforms.width {
        return;
    }
    let pos = vec2f(f32(id.x), f32(id.y));
    if distance(pos, imm.coord) < abs(imm.size) {
        let index = offset_index(id.x, id.y, 0, 0);
        if imm.size < 0 {
            snad_mut[index] = 0;
        } else {
            snad_mut[index] = u32(-1i);
        }
    }
}
