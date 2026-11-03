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

struct ReflectionHit { index:u32, distance:f32, uv:vec2f, coverage:f32 };

fn reflection_sample(hit:ReflectionHit,lod:f32) -> vec4f {
    if hit.index>=7u { return vec4f(0); }
    let poster=scene.posters[hit.index];
    // Coherence fades with height above the contact, as well as path length.
    // The concrete catches light, not an intact second cover in the foreground.
    let attenuation=exp(-hit.distance*0.18-max(1.0-hit.uv.y,0.0)*poster.shape.y*0.85);
    return vec4f(art_color(poster,hit.uv,lod+min(hit.distance*0.35,2.0))*attenuation,hit.coverage*poster.shape.w);
}

fn trace_art(origin: vec3f, direction: vec3f, lod: f32) -> vec3f {
    var first=ReflectionHit(7u,100000.0,vec2f(0),0.0);
    var second=first;
    for (var i=0u; i<u32(scene.viewport.w); i++) {
        let poster = scene.posters[i];
        let tangent=vec3f(poster.basis.x,0,-poster.basis.y);
        let normal=vec3f(poster.basis.y,0,poster.basis.x);
        let denominator=dot(direction,normal);
        if abs(denominator)<0.00001 { continue; }
        let distance=dot(poster.center.xyz-origin,normal)/denominator;
        if distance<=0.001 || distance>=second.distance { continue; }
        let local=origin+direction*distance-poster.center.xyz;
        let uv=vec2f(dot(local,tangent)/(2.0*poster.shape.x)+0.5,1.0-local.y/poster.shape.y);
        let edge=min(min(uv.x,1.0-uv.x),min(uv.y,1.0-uv.y));
        let feather=smoothstep(-0.16,0.22,edge);
        if feather>0.001 {
            let hit=ReflectionHit(i,distance,uv,feather);
            if distance<first.distance { second=first; first=hit; }
            else { second=hit; }
        }
    }
    // Feathered reflection edges must reveal the next surface, not discard it:
    // treating a 1%-covered edge as opaque creates dark trapezoid-shaped holes.
    // Only two texture samples per ray, even when many cover bounds overlap.
    let near=reflection_sample(first,lod);
    let far=reflection_sample(second,lod);
    return near.rgb*near.a+far.rgb*far.a*(1.0-near.a);
}

fn floor_color(point: vec3f, incoming: vec3f) -> vec3f {
    // Level polished concrete: broad uneven polish changes both reflection
    // sharpness and energy. Fine trowel marks/pitting break up the light, not
    // the geometry. No animated noise or water-like rippled surface normals.
    let surface=textureSampleLevel(material,material_sampler,point.xz*0.065+vec2f(0.17,0.31),0.0);
    let wear=textureSampleLevel(material,material_sampler,point.xz*vec2f(0.42,1.65),0.0);
    let micro=textureSampleLevel(material,material_sampler,point.xz*vec2f(1.7,5.8),0.0).b;
    let roughness=clamp(surface.r+(wear.b-0.5)*0.08,0.20,0.76);
    let polish=surface.g;
    let normal = vec3f(0,1,0);
    let outgoing = reflect(incoming,normal);
    let axis_x = normalize(cross(outgoing,vec3f(0,1,0)));
    let axis_y = normalize(cross(axis_x,outgoing));
    var reflected=vec3f(0);
    // A fixed, temporally stable roughness cone: no frame-to-frame sparkling.
    let cone=array<vec2f,4>(vec2f(-0.58,-0.24),vec2f(0.24,-0.58),vec2f(0.58,0.24),vec2f(-0.24,0.58));
    for (var sample_index=0u; sample_index<4u; sample_index++) {
        let ray=normalize(outgoing+(axis_x*cone[sample_index].x+axis_y*cone[sample_index].y)*roughness*0.15);
        reflected += trace_art(point+normal*0.003,ray,2.5+roughness*5.2);
    }
    reflected/=4.0;
    let fresnel=0.055+0.945*pow(1.0-clamp(dot(-incoming,normal),0.0,1.0),5.0);
    let distance=length(scene.eye.xyz-point);
    let grain=(0.60+surface.a*0.45)*(0.66+wear.b*0.47)*(0.83+micro*0.30);
    var base=vec3f(0.0038,0.0033,0.0029)*grain;
    var illumination=vec3f(0); var contact=0.0;
    for (var i=0u; i<u32(scene.viewport.w); i++) {
        let poster=scene.posters[i];
        let delta=point-poster.center.xyz;
        let tangent=vec3f(poster.basis.x,0,-poster.basis.y);
        let normal_p=vec3f(poster.basis.y,0,poster.basis.x);
        let across=dot(delta,tangent); let along=dot(delta,normal_p);
        let edge=max(abs(across)-poster.shape.x,0.0);
        let falloff=exp(-edge*edge*1.3-along*along*0.48);
        illumination += poster.tint.rgb * falloff * 0.029 * poster.shape.w;
        contact=max(contact,exp(-abs(along)*13.0)*(1.0-smoothstep(poster.shape.x,poster.shape.x+0.25,abs(across)))*poster.shape.w);
    }
    base=(base+illumination*grain)*(1.0-contact*0.72);
    let reflection_strength=fresnel*mix(0.20,1.18,polish)*exp(-max(distance-8.0,0.0)*0.025)*grain;
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
