#[compute]
#version 450

layout(rgba16f, binding = 0) uniform readonly image2D inputImg;
layout(rgba16f, binding = 1) uniform writeonly image2D outputImg;

const mat4 RGB_TO_YUV = mat4(
        0.299, 0.587, 0.114, 0.0,
        -0.14713, -0.28886, 0.436, 0.0,
        0.615, -0.51499, -0.10001, 0.0,
        0.0, 0.0, 0.0, 1.0
    );

layout(local_size_x = 16, local_size_y = 16) in;
void main() {
    ivec2 pixel_coords = ivec2(gl_GlobalInvocationID.xy);
    ivec2 image_size = imageSize(inputImg);

    if (pixel_coords.x >= image_size.x || pixel_coords.y >= image_size.y) {
        return;
    }

    vec4 rgba = imageLoad(inputImg, pixel_coords);
    vec4 yuva = RGB_TO_YUV * rgba;

    imageStore(outputImg, pixel_coords, yuva);
}
