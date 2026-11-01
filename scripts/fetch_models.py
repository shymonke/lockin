"""Rebuild the ONNX files in models/ that come from upstream releases.

    python -m venv .venv  (Python 3.11)
    .venv/Scripts/pip install tensorflow-cpu==2.15.1 tf2onnx "onnx==1.16.2" "numpy<2"
    .venv/Scripts/python scripts/fetch_models.py

- MediaPipe Face Landmarker (Apache-2.0): face_landmarker.task is a zip of TFLite models;
  the landmark and blendshape models are converted to ONNX with tf2onnx.
- RTMDet-tiny, COCO (Apache-2.0, OpenMMLab): an ONNX export of MMDetection's
  rtmdet_tiny_8xb32-300e_coco. It ends in NMS; the graph is cut at the per-anchor class scores,
  which is all LockIn reads.

YuNet (yunet.onnx) is taken unchanged from OpenCV Zoo and is not rebuilt here.
"""

import hashlib
import io
import pathlib
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

import onnx
from onnx import utils

MODELS = pathlib.Path(__file__).resolve().parent.parent / "models"
FACE_LANDMARKER = (
    "https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task"
)
RTMDET = "https://huggingface.co/bukuroo/RTMDet-ONNX/resolve/main/rtmdet-tiny-coco.onnx"
# Sigmoid class scores, shape [1, anchors, 80], just before the NMS block.
RTMDET_SCORES = "1035"


def fetch(url: str) -> bytes:
    print(f"fetch {url}")
    with urllib.request.urlopen(url) as r:
        return r.read()


def tflite_to_onnx(tflite: bytes, out: pathlib.Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        src = pathlib.Path(tmp) / "model.tflite"
        src.write_bytes(tflite)
        subprocess.run(
            [sys.executable, "-m", "tf2onnx.convert", "--tflite", str(src), "--output", str(out), "--opset", "17"],
            check=True,
        )


def rtmdet_scores(full: bytes, out: pathlib.Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        src = pathlib.Path(tmp) / "full.onnx"
        src.write_bytes(full)
        utils.extract_model(str(src), str(out), ["input"], [RTMDET_SCORES])
    m = onnx.load(str(out))
    # Fixed 640x640 shapes and initializers listed as inputs would pin the size; LockIn runs 416x416.
    del m.graph.value_info[:]
    inits = {i.name for i in m.graph.initializer}
    inputs = [i for i in m.graph.input if i.name not in inits]
    del m.graph.input[:]
    m.graph.input.extend(inputs)
    for t in list(m.graph.input) + list(m.graph.output):
        t.type.tensor_type.ClearField("shape")
    onnx.save(m, str(out))


def main() -> None:
    task = zipfile.ZipFile(io.BytesIO(fetch(FACE_LANDMARKER)))
    tflite_to_onnx(task.read("face_landmarks_detector.tflite"), MODELS / "face_landmarks.onnx")
    tflite_to_onnx(task.read("face_blendshapes.tflite"), MODELS / "face_blendshapes.onnx")
    rtmdet_scores(fetch(RTMDET), MODELS / "rtmdet.onnx")
    for name in ("face_landmarks.onnx", "face_blendshapes.onnx", "rtmdet.onnx"):
        data = (MODELS / name).read_bytes()
        print(f"{name} {len(data)} bytes sha256 {hashlib.sha256(data).hexdigest()}")


if __name__ == "__main__":
    main()
