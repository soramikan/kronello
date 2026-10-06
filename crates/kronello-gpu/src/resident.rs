//! Device-bound image ownership at the native media boundary.
use crate::{GpuContext, GpuError, WorkingSpace};

#[derive(Debug, Clone)]
pub struct ResidentImage {
    texture: wgpu::Texture,
    identity: std::sync::Arc<()>,
    working: WorkingSpace,
}
impl ResidentImage {
    /// Allocate on the recorded device; no foreign texture can forge ownership.
    /// Producers initialize through this device queue before scene consumption.
    pub fn allocate(
        gpu: &GpuContext,
        size: [u32; 2],
        working: WorkingSpace,
    ) -> Result<Self, GpuError> {
        let texture = gpu.texture(
            size[0],
            size[1],
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        )?;
        Ok(Self {
            texture,
            identity: gpu.identity.clone(),
            working,
        })
    }
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn working_space(&self) -> WorkingSpace {
        self.working
    }
    pub fn validate(&self) -> Result<(), GpuError> {
        if self.texture.format() != wgpu::TextureFormat::Rgba16Float
            || self.texture.dimension() != wgpu::TextureDimension::D2
            || self.texture.depth_or_array_layers() != 1
            || self.texture.sample_count() != 1
            || !self
                .texture
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        {
            return Err(GpuError::InvalidInput("resident image format/usage"));
        }
        Ok(())
    }
    pub fn validate_for(
        &self,
        gpu: &GpuContext,
        size: [u32; 2],
        working: WorkingSpace,
    ) -> Result<(), GpuError> {
        self.validate()?;
        if !std::sync::Arc::ptr_eq(&self.identity, &gpu.identity) {
            return Err(GpuError::InvalidInput(
                "resident image belongs to another device",
            ));
        }
        if [self.texture.width(), self.texture.height()] != size || self.working != working {
            return Err(GpuError::InvalidInput(
                "resident image dimensions/working space",
            ));
        }
        Ok(())
    }
}
