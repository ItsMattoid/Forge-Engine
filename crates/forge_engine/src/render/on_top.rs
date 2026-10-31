//! The material of `Highlight`: a plain unlit colour (the standard material, see `classes/effects/highlight.rs`) that can also be drawn
//! "always on top", where it is not hidden by whatever is in front of it. Bevy's materials have no switch for that, so the depth
//! test is changed when the pipeline is made.

use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline, StandardMaterial},
    prelude::*,
    render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError},
};

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default, PartialEq)]
#[bind_group_data(OnTopKey)]
pub struct OnTop {
    /// Drawn over everything, whatever is in front of it.
    pub always: bool,
}

/// What the pipeline depends on.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OnTopKey {
    always: bool,
}

impl From<&OnTop> for OnTopKey {
    fn from(on_top: &OnTop) -> OnTopKey {
        OnTopKey { always: on_top.always }
    }
}

impl MaterialExtension for OnTop {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.always {
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_compare = Some(CompareFunction::Always);
                depth.depth_write_enabled = Some(false);
            }
        }
        Ok(())
    }
}

/// What a highlight is drawn with.
pub type HighlightMaterial = ExtendedMaterial<StandardMaterial, OnTop>;
