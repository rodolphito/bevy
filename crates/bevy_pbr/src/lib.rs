// FIXME(15321): solve CI failures, then replace with `#![expect()]`.
#![allow(missing_docs, reason = "Not all docs are written yet, see #3492.")]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![deny(unsafe_code)]
#![doc(
    html_logo_url = "https://bevyengine.org/assets/icon.png",
    html_favicon_url = "https://bevyengine.org/assets/icon.png"
)]

extern crate alloc;

#[cfg(feature = "meshlet")]
mod meshlet;
pub mod wireframe;

/// Experimental features that are not yet finished. Please report any issues you encounter!
///
/// Expect bugs, missing features, compatibility issues, low performance, and/or future breaking changes.
#[cfg(feature = "meshlet")]
pub mod experimental {
    /// Render high-poly 3d meshes using an efficient GPU-driven method.
    /// See [`MeshletPlugin`](meshlet::MeshletPlugin) and [`MeshletMesh`](meshlet::MeshletMesh) for details.
    pub mod meshlet {
        pub use crate::meshlet::*;
    }
}

mod bundle;
mod cluster;
pub mod deferred;
mod extended_material;
mod fog;
mod light;
mod light_probe;
mod lightmap;
mod material;
mod mesh_material;
mod parallax;
mod pbr_material;
mod prepass;
mod render;
mod ssao;
mod ssr;
mod volumetric_fog;

use core::marker::PhantomData;
use std::path::PathBuf;

pub use bundle::*;
pub use cluster::*;
pub use extended_material::*;
pub use fog::*;
pub use light::*;
pub use light_probe::*;
pub use lightmap::*;
pub use material::*;
pub use mesh_material::*;
pub use parallax::*;
pub use pbr_material::*;
pub use prepass::*;
pub use render::*;
pub use ssao::*;
pub use ssr::*;
#[allow(deprecated)]
pub use volumetric_fog::{
    FogVolume, FogVolumeBundle, VolumetricFog, VolumetricFogPlugin, VolumetricFogSettings,
    VolumetricLight,
};

/// The PBR prelude.
///
/// This includes the most common types in this crate, re-exported for your convenience.
#[expect(deprecated)]
pub mod prelude {
    #[doc(hidden)]
    pub use crate::{
        bundle::{
            DirectionalLightBundle, MaterialMeshBundle, PbrBundle, PointLightBundle,
            SpotLightBundle,
        },
        fog::{DistanceFog, FogFalloff},
        light::{light_consts, AmbientLight, DirectionalLight, PointLight, SpotLight},
        light_probe::{
            environment_map::{EnvironmentMapLight, ReflectionProbeBundle},
            LightProbe,
        },
        material::{Material, MaterialPlugin},
        mesh_material::MeshMaterial3d,
        parallax::ParallaxMappingMethod,
        pbr_material::StandardMaterial,
        ssao::ScreenSpaceAmbientOcclusionPlugin,
    };
}

pub mod graph {
    use bevy_render::render_graph::RenderLabel;

    #[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
    pub enum NodePbr {
        /// Label for the shadow pass node.
        ShadowPass,
        /// Label for the screen space ambient occlusion render node.
        ScreenSpaceAmbientOcclusion,
        DeferredLightingPass,
        /// Label for the volumetric lighting pass.
        VolumetricFog,
        /// Label for the compute shader instance data building pass.
        GpuPreprocess,
        /// Label for the screen space reflections pass.
        ScreenSpaceReflections,
    }
}

use crate::{deferred::DeferredPbrLightingPlugin, graph::NodePbr};
use bevy_app::prelude::*;
use bevy_asset::{AssetApp, AssetPath, Assets, Handle};
use bevy_color::{Color, LinearRgba};
use bevy_core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy_ecs::prelude::*;
use bevy_render::{
    alpha::AlphaMode,
    camera::{
        CameraProjection, CameraUpdateSystem, OrthographicProjection, PerspectiveProjection,
        Projection,
    },
    extract_component::ExtractComponentPlugin,
    extract_resource::ExtractResourcePlugin,
    load_and_forget_shader,
    render_asset::prepare_assets,
    render_graph::RenderGraph,
    render_resource::ShaderRef,
    texture::{GpuImage, Image},
    view::{check_visibility, VisibilitySystems},
    ExtractSchedule, Render, RenderApp, RenderSet,
};
use bevy_transform::TransformSystem;

fn shader_ref(path: PathBuf) -> ShaderRef {
    ShaderRef::Path(AssetPath::from_path_buf(path).with_source("embedded"))
}

/// Sets up the entire PBR infrastructure of bevy.
pub struct PbrPlugin {
    /// Controls if the prepass is enabled for the [`StandardMaterial`].
    /// For more information about what a prepass is, see the [`bevy_core_pipeline::prepass`] docs.
    pub prepass_enabled: bool,
    /// Controls if [`DeferredPbrLightingPlugin`] is added.
    pub add_default_deferred_lighting_plugin: bool,
    /// Controls if GPU [`MeshUniform`] building is enabled.
    ///
    /// This requires compute shader support and so will be forcibly disabled if
    /// the platform doesn't support those.
    pub use_gpu_instance_buffer_builder: bool,
}

impl Default for PbrPlugin {
    fn default() -> Self {
        Self {
            prepass_enabled: true,
            add_default_deferred_lighting_plugin: true,
            use_gpu_instance_buffer_builder: true,
        }
    }
}

impl Plugin for PbrPlugin {
    fn build(&self, app: &mut App) {
        load_and_forget_shader!(app, "render/pbr_types.wgsl");
        load_and_forget_shader!(app, "render/pbr_bindings.wgsl");
        load_and_forget_shader!(app, "render/utils.wgsl");
        load_and_forget_shader!(app, "render/clustered_forward.wgsl");
        load_and_forget_shader!(app, "render/pbr_lighting.wgsl");
        load_and_forget_shader!(app, "render/pbr_transmission.wgsl");
        load_and_forget_shader!(app, "render/shadows.wgsl");
        load_and_forget_shader!(app, "deferred/pbr_deferred_types.wgsl");
        load_and_forget_shader!(app, "deferred/pbr_deferred_functions.wgsl");
        load_and_forget_shader!(app, "render/shadow_sampling.wgsl");
        load_and_forget_shader!(app, "render/pbr_functions.wgsl");
        load_and_forget_shader!(app, "render/rgb9e5.wgsl");
        load_and_forget_shader!(app, "render/pbr_ambient.wgsl");
        load_and_forget_shader!(app, "render/pbr_fragment.wgsl");
        load_and_forget_shader!(app, "render/pbr.wgsl");
        load_and_forget_shader!(app, "render/pbr_prepass_functions.wgsl");
        load_and_forget_shader!(app, "render/pbr_prepass.wgsl");
        load_and_forget_shader!(app, "render/parallax_mapping.wgsl");
        load_and_forget_shader!(app, "render/view_transformations.wgsl");
        // Setup dummy shaders for when MeshletPlugin is not used to prevent shader import errors.
        load_and_forget_shader!(app, "meshlet/dummy_visibility_buffer_resolve.wgsl");

        app.register_asset_reflect::<StandardMaterial>()
            .register_type::<AmbientLight>()
            .register_type::<CascadeShadowConfig>()
            .register_type::<Cascades>()
            .register_type::<CascadesVisibleEntities>()
            .register_type::<VisibleMeshEntities>()
            .register_type::<ClusterConfig>()
            .register_type::<CubemapVisibleEntities>()
            .register_type::<DirectionalLight>()
            .register_type::<DirectionalLightShadowMap>()
            .register_type::<NotShadowCaster>()
            .register_type::<NotShadowReceiver>()
            .register_type::<PointLight>()
            .register_type::<PointLightShadowMap>()
            .register_type::<SpotLight>()
            .register_type::<DistanceFog>()
            .register_type::<ShadowFilteringMethod>()
            .init_resource::<AmbientLight>()
            .init_resource::<GlobalVisibleClusterableObjects>()
            .init_resource::<DirectionalLightShadowMap>()
            .init_resource::<PointLightShadowMap>()
            .register_type::<DefaultOpaqueRendererMethod>()
            .init_resource::<DefaultOpaqueRendererMethod>()
            .add_plugins((
                MeshRenderPlugin {
                    use_gpu_instance_buffer_builder: self.use_gpu_instance_buffer_builder,
                },
                MaterialPlugin::<StandardMaterial> {
                    prepass_enabled: self.prepass_enabled,
                    ..Default::default()
                },
                ScreenSpaceAmbientOcclusionPlugin,
                ExtractResourcePlugin::<AmbientLight>::default(),
                FogPlugin,
                ExtractResourcePlugin::<DefaultOpaqueRendererMethod>::default(),
                ExtractComponentPlugin::<ShadowFilteringMethod>::default(),
                LightmapPlugin,
                LightProbePlugin,
                PbrProjectionPlugin::<Projection>::default(),
                PbrProjectionPlugin::<PerspectiveProjection>::default(),
                PbrProjectionPlugin::<OrthographicProjection>::default(),
                GpuMeshPreprocessPlugin {
                    use_gpu_instance_buffer_builder: self.use_gpu_instance_buffer_builder,
                },
                VolumetricFogPlugin,
                ScreenSpaceReflectionsPlugin,
            ))
            .configure_sets(
                PostUpdate,
                (
                    SimulationLightSystems::AddClusters,
                    SimulationLightSystems::AssignLightsToClusters,
                )
                    .chain(),
            )
            .configure_sets(
                PostUpdate,
                SimulationLightSystems::UpdateDirectionalLightCascades
                    .ambiguous_with(SimulationLightSystems::UpdateDirectionalLightCascades),
            )
            .configure_sets(
                PostUpdate,
                SimulationLightSystems::CheckLightVisibility
                    .ambiguous_with(SimulationLightSystems::CheckLightVisibility),
            )
            .add_systems(
                PostUpdate,
                (
                    add_clusters
                        .in_set(SimulationLightSystems::AddClusters)
                        .after(CameraUpdateSystem),
                    assign_objects_to_clusters
                        .in_set(SimulationLightSystems::AssignLightsToClusters)
                        .after(TransformSystem::TransformPropagate)
                        .after(VisibilitySystems::CheckVisibility)
                        .after(CameraUpdateSystem),
                    clear_directional_light_cascades
                        .in_set(SimulationLightSystems::UpdateDirectionalLightCascades)
                        .after(TransformSystem::TransformPropagate)
                        .after(CameraUpdateSystem),
                    update_directional_light_frusta
                        .in_set(SimulationLightSystems::UpdateLightFrusta)
                        // This must run after CheckVisibility because it relies on `ViewVisibility`
                        .after(VisibilitySystems::CheckVisibility)
                        .after(TransformSystem::TransformPropagate)
                        .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                        // We assume that no entity will be both a directional light and a spot light,
                        // so these systems will run independently of one another.
                        // FIXME: Add an archetype invariant for this https://github.com/bevyengine/bevy/issues/1481.
                        .ambiguous_with(update_spot_light_frusta),
                    update_point_light_frusta
                        .in_set(SimulationLightSystems::UpdateLightFrusta)
                        .after(TransformSystem::TransformPropagate)
                        .after(SimulationLightSystems::AssignLightsToClusters),
                    update_spot_light_frusta
                        .in_set(SimulationLightSystems::UpdateLightFrusta)
                        .after(TransformSystem::TransformPropagate)
                        .after(SimulationLightSystems::AssignLightsToClusters),
                    check_visibility::<WithLight>.in_set(VisibilitySystems::CheckVisibility),
                    (
                        check_dir_light_mesh_visibility,
                        check_point_light_mesh_visibility,
                    )
                        .in_set(SimulationLightSystems::CheckLightVisibility)
                        .after(VisibilitySystems::CalculateBounds)
                        .after(TransformSystem::TransformPropagate)
                        .after(SimulationLightSystems::UpdateLightFrusta)
                        // NOTE: This MUST be scheduled AFTER the core renderer visibility check
                        // because that resets entity `ViewVisibility` for the first view
                        // which would override any results from this otherwise
                        .after(VisibilitySystems::CheckVisibility),
                ),
            );

        if self.add_default_deferred_lighting_plugin {
            app.add_plugins(DeferredPbrLightingPlugin);
        }

        // Initialize the default material.
        app.world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .insert(
                &Handle::<StandardMaterial>::default(),
                StandardMaterial {
                    base_color: Color::WHITE,
                    ..Default::default()
                },
            );

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        // Extract the required data from the main world
        render_app
            .add_systems(
                ExtractSchedule,
                (
                    extract_clusters,
                    extract_lights,
                    extract_default_materials.after(clear_material_instances::<StandardMaterial>),
                ),
            )
            .add_systems(
                Render,
                (
                    prepare_lights
                        .in_set(RenderSet::ManageViews)
                        .after(prepare_assets::<GpuImage>),
                    prepare_clusters.in_set(RenderSet::PrepareResources),
                ),
            )
            .init_resource::<LightMeta>();

        render_app.world_mut().observe(add_light_view_entities);
        render_app.world_mut().observe(remove_light_view_entities);

        let shadow_pass_node = ShadowPassNode::new(render_app.world_mut());
        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        let draw_3d_graph = graph.get_sub_graph_mut(Core3d).unwrap();
        draw_3d_graph.add_node(NodePbr::ShadowPass, shadow_pass_node);
        draw_3d_graph.add_node_edge(NodePbr::ShadowPass, Node3d::StartMainPass);
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        // Extract the required data from the main world
        render_app
            .init_resource::<ShadowSamplers>()
            .init_resource::<GlobalClusterableObjectMeta>();
    }
}

/// [`CameraProjection`] specific PBR functionality.
pub struct PbrProjectionPlugin<T: CameraProjection + Component>(PhantomData<T>);
impl<T: CameraProjection + Component> Plugin for PbrProjectionPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            build_directional_light_cascades::<T>
                .in_set(SimulationLightSystems::UpdateDirectionalLightCascades)
                .after(clear_directional_light_cascades),
        );
    }
}
impl<T: CameraProjection + Component> Default for PbrProjectionPlugin<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}
