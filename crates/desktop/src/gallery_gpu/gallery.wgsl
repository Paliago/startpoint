// Actual world-space camera rays, upright artwork planes and a material floor.
// Analytic intersections make this tiny scene independent of hardware ray tracing.
struct Poster { center: vec4f, shape: vec4f, tint: vec4f, basis: vec4f };
struct Scene {
    viewport: vec4f, eye: vec4f, forward: vec4f, up: vec4f, optics: vec4f,
    posters: array<Poster, 7>,
};
@group(0) @binding(0) var<uniform> scene: Scene;
@group(0) @binding(1) var artwork: texture_2d_array<f32>;
@group(0) @binding(2) var art_sampler: sampler;
@group(0) @binding(3) var material: texture_2d<f32>;
@group(0) @binding(4) var material_sampler: sampler;

@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

fn environment(direction: vec3f) -> vec3f {
    let base = vec3f(0.00368,0.00304,0.00273);
    let haze = exp(-pow(direction.x*1.1,2.0)-pow(direction.y*2.5,2.0));
    return base * (0.73+0.27*haze);
}

// x = distance, yz = image UV. A negative distance means no intersection.
fn intersect(origin: vec3f, direction: vec3f, poster: Poster) -> vec3f {
    let tangent = vec3f(poster.basis.x,0.0,-poster.basis.y);
    let normal = vec3f(poster.basis.y,0.0,poster.basis.x);
    let denominator = dot(direction,normal);
    if abs(denominator) < 0.00001 { return vec3f(-1.0,0.0,0.0); }
    let t = dot(poster.center.xyz-origin,normal)/denominator;
    if t <= 0.001 { return vec3f(-1.0,0.0,0.0); }
    let local = origin+direction*t-poster.center.xyz;
    let x = dot(local,tangent);
    if abs(x) > poster.shape.x || local.y < 0.0 || local.y > poster.shape.y { return vec3f(-1.0,0.0,0.0); }
    return vec3f(t, x/(2.0*poster.shape.x)+0.5, 1.0-local.y/poster.shape.y);
}

fn art_color(poster: Poster, uv: vec2f, lod: f32) -> vec3f {
    let pixel = textureSampleLevel(artwork,art_sampler,clamp(uv,vec2f(0),vec2f(1)),i32(poster.center.w),lod);
    let edge = min(min(uv.x,1.0-uv.x),min(uv.y,1.0-uv.y));
    let rim = smoothstep(0.0,0.002,edge);
    return mix(pixel.rgb*0.58,pixel.rgb,rim)*(1.0-poster.tint.w*0.7);
}

fn trace_art(origin: vec3f, direction: vec3f, lod: f32) -> vec4f {
    var nearest = 100000.0; var result = vec4f(0);
    for (var i=0u; i<u32(scene.viewport.w); i++) {
        let poster = scene.posters[i];
        let tangent=vec3f(poster.basis.x,0,-poster.basis.y);
        let normal=vec3f(poster.basis.y,0,poster.basis.x);
        let denominator=dot(direction,normal);
        if abs(denominator)<0.00001 { continue; }
        let distance=dot(poster.center.xyz-origin,normal)/denominator;
        if distance<=0.001 || distance>=nearest { continue; }
        let local=origin+direction*distance-poster.center.xyz;
        let uv=vec2f(dot(local,tangent)/(2.0*poster.shape.x)+0.5,1.0-local.y/poster.shape.y);
        let edge=min(min(uv.x,1.0-uv.x),min(uv.y,1.0-uv.y));
        let feather=smoothstep(-0.10,0.13,edge);
        if feather>0.001 {
            nearest=distance;
            // Rough surface detail loses coherence with reflected path length.
            // Keep strong contact light without a second full-bright poster
            // stretching into the title and native controls in the foreground.
            result=vec4f(art_color(poster,uv,lod+min(distance*0.35,2.0))*exp(-distance*0.30),poster.shape.w*feather);
        }
    }
    return result;
}

fn floor_color(point: vec3f, incoming: vec3f) -> vec3f {
    // Dry, level, satin-finished floor. Grain changes the albedo only, never
    // the normal: rippled normals turn reflected light into water/ice.
    // A tiny precomputed mipmapped material replaces per-pixel procedural noise.
    let grain_sample = textureSampleLevel(material,material_sampler,point.xz*vec2f(0.3,4.0),0.0).r;
    let roughness = 0.48;
    let normal = vec3f(0,1,0);
    let outgoing = reflect(incoming,normal);
    let axis_x = normalize(cross(outgoing,vec3f(0,1,0)));
    let axis_y = normalize(cross(axis_x,outgoing));
    var reflected=vec3f(0);
    // A fixed, temporally stable roughness cone: no frame-to-frame sparkling.
    let cone=array<vec2f,4>(vec2f(-0.58,-0.24),vec2f(0.24,-0.58),vec2f(0.58,0.24),vec2f(-0.24,0.58));
    for (var sample_index=0u; sample_index<4u; sample_index++) {
        let ray=normalize(outgoing+(axis_x*cone[sample_index].x+axis_y*cone[sample_index].y)*roughness*0.12);
        let hit=trace_art(point+normal*0.003,ray,4.7);
        reflected += hit.rgb*hit.a;
    }
    reflected/=4.0;
    let fresnel=0.055+0.945*pow(1.0-clamp(dot(-incoming,normal),0.0,1.0),5.0);
    let distance=length(scene.eye.xyz-point);
    let grain=0.60+grain_sample*0.65;
    var base=vec3f(0.0035,0.0029,0.0025)*grain;
    var illumination=vec3f(0); var contact=0.0;
    for (var i=0u; i<u32(scene.viewport.w); i++) {
        let poster=scene.posters[i];
        let delta=point-poster.center.xyz;
        let tangent=vec3f(poster.basis.x,0,-poster.basis.y);
        let normal_p=vec3f(poster.basis.y,0,poster.basis.x);
        let across=dot(delta,tangent); let along=dot(delta,normal_p);
        let edge=max(abs(across)-poster.shape.x,0.0);
        let falloff=exp(-edge*edge*1.3-along*along*0.48);
        illumination += poster.tint.rgb * falloff * 0.035 * poster.shape.w;
        contact=max(contact,exp(-abs(along)*13.0)*(1.0-smoothstep(poster.shape.x,poster.shape.x+0.25,abs(across)))*poster.shape.w);
    }
    base=(base+illumination*grain)*(1.0-contact*0.72);
    let reflection_strength=fresnel*0.55*exp(-max(distance-8.0,0.0)*0.025)*grain;
    let result=base+reflected*reflection_strength;
    let atmosphere=exp(-max(distance-16.0,0.0)*0.055);
    return mix(environment(incoming),result,atmosphere);
}

@fragment fn fs(@builtin(position) pixel: vec4f) -> @location(0) vec4f {
    let screen=vec2f(pixel.x/scene.viewport.x*2.0-1.0,1.0-pixel.y/scene.viewport.y*2.0);
    let direction=normalize(scene.forward.xyz+vec3f(1,0,0)*screen.x*scene.optics.x*scene.viewport.z+scene.up.xyz*(screen.y-scene.optics.y)*scene.optics.x);
    var color=environment(direction);
    var floor_distance=100000.0;
    if direction.y < -0.0001 {
        floor_distance=-scene.eye.y/direction.y;
    }
    var nearest=floor_distance;
    var nearest_index=7u;
    var nearest_uv=vec2f(0);
    for (var i=0u; i<u32(scene.viewport.w); i++) {
        let poster=scene.posters[i];
        let hit=intersect(scene.eye.xyz,direction,poster);
        if hit.x > 0.0 && hit.x < nearest {
            nearest=hit.x;
            nearest_index=i;
            nearest_uv=hit.yz;
        }
    }
    let vignette=1.0-0.10*smoothstep(0.6,1.5,length(screen*vec2f(0.8,0.65)));
    // Most artwork pixels are opaque: never trace the invisible floor behind
    // them. Only the exposed lower strip needs rough reflection shading.
    if nearest_index < 7u && scene.posters[nearest_index].shape.w >= 0.999 {
        let lod=max(0.0,log2(768.0/(scene.viewport.y*0.55)));
        return vec4f(art_color(scene.posters[nearest_index],nearest_uv,lod)*vignette,1.0);
    }
    if floor_distance < 200.0 { color=floor_color(scene.eye.xyz+direction*floor_distance,direction); }
    if nearest_index < 7u {
        let poster=scene.posters[nearest_index];
        let lod=max(0.0,log2(768.0/(scene.viewport.y*0.55)));
        color=mix(color,art_color(poster,nearest_uv,lod),poster.shape.w);
    }
    return vec4f(max(color*vignette,vec3f(0)),1.0);
}
