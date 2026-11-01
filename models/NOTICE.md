# Model notices

The ONNX files here are compiled into `lockin.exe`. `scripts/fetch_models.py` rebuilds the three
converted ones from their upstream releases.

| File | Model | Source | License |
|---|---|---|---|
| `yunet.onnx` | YuNet face detector, input shape relaxed for 256x192 | [OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet) | MIT, Copyright (c) 2020 Shiqi Yu; see `LICENSE-YUNET-MIT` |
| `face_landmarks.onnx` | MediaPipe Face Mesh V2: 478 landmarks incl. irises (`face_landmarks_detector.tflite` from `face_landmarker.task`, float16 v1), converted with tf2onnx | [Google AI Edge, Face Landmarker](https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker) | Apache-2.0, Copyright Google LLC; see `LICENSE-APACHE-2.0` |
| `face_blendshapes.onnx` | MediaPipe blendshapes model (`face_blendshapes.tflite` from the same bundle), converted with tf2onnx | as above | Apache-2.0, Copyright Google LLC; see `LICENSE-APACHE-2.0` |
| `rtmdet.onnx` | RTMDet-tiny, COCO (`rtmdet_tiny_8xb32-300e_coco`), cut before NMS at the class scores | [MMDetection](https://github.com/open-mmlab/mmdetection/tree/main/configs/rtmdet), ONNX export by [bukuroo/RTMDet-ONNX](https://huggingface.co/bukuroo/RTMDet-ONNX) | Apache-2.0, Copyright OpenMMLab; see `LICENSE-APACHE-2.0` |

Changes from upstream: format conversion (TFLite to ONNX), removal of the NMS tail and fixed
shape annotations from RTMDet. Weights are unchanged.
