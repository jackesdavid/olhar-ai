# Sentinel AI sidecar — CPU image (works on RunPod CPU or low-end GPU endpoints).
# For a CUDA/GPU build, swap the base for e.g. `runpod/pytorch:*-cu121` and let
# ultralytics pick up the GPU automatically.
FROM python:3.11-slim

# OpenCV (pulled in by ultralytics) needs these shared libs at runtime.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgl1 libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Pre-download the YOLO weights into the image so workers don't fetch them on
# the first request (keeps cold starts fast).
RUN python -c "from ultralytics import YOLO; YOLO('yolov8n.pt')"

COPY handler.py .

# RunPod serverless entrypoint (also serves /runsync locally with --rp_serve_api).
CMD ["python", "-u", "handler.py"]
