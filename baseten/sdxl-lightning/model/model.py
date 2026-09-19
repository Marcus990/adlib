import base64
import time
from io import BytesIO
from typing import Dict

import torch
from diffusers import (
    EulerDiscreteScheduler,
    StableDiffusionXLPipeline,
    UNet2DConditionModel,
)
from huggingface_hub import hf_hub_download
from PIL import Image
from safetensors.torch import load_file

GUIDANCE_SCALE = 0.0
BASE = "stabilityai/stable-diffusion-xl-base-1.0"
REPO = "ByteDance/SDXL-Lightning"
CKPT = "sdxl_lightning_4step_unet.safetensors"  # Use the correct ckpt for your step setting!


class Model:
    def __init__(self, **kwargs):
        self.model = None

    def load(self):
        unet = UNet2DConditionModel.from_config(BASE, subfolder="unet").to(
            "cuda", torch.float16
        )
        unet.load_state_dict(load_file(hf_hub_download(REPO, CKPT), device="cuda"))
        pipe = StableDiffusionXLPipeline.from_pretrained(
            BASE, unet=unet, torch_dtype=torch.float16, variant="fp16"
        ).to("cuda")

        # Ensure sampler uses "trailing" timesteps.
        pipe.scheduler = EulerDiscreteScheduler.from_config(
            pipe.scheduler.config, timestep_spacing="trailing"
        )

        self.model = pipe

    def convert_to_b64(self, image: Image) -> str:
        buffered = BytesIO()
        image.save(buffered, format="JPEG")
        img_b64 = base64.b64encode(buffered.getvalue()).decode("utf-8")
        return img_b64

    def predict(self, model_input: Dict) -> Dict:
        prompt = model_input.get("prompt")
        num_steps = 4
        # Default 512x512 for lowest latency; both dims must be multiples of 8.
        width = model_input.get("width", 512)
        height = model_input.get("height", 512)
        t0 = time.perf_counter()
        output_image = self.model(
            prompt,
            num_inference_steps=num_steps,
            guidance_scale=GUIDANCE_SCALE,
            width=width,
            height=height,
        ).images[0]
        generation_time_s = time.perf_counter() - t0
        return {
            "result": self.convert_to_b64(output_image),
            "generation_time_s": round(generation_time_s, 3),
        }
