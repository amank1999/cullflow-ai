# Third-party model assets

## face_detector_rfb320.onnx

- **Source**: [Ultra-Light-Fast-Generic-Face-Detector-1MB](https://github.com/Linzaer/Ultra-Light-Fast-Generic-Face-Detector-1MB)
  by Linzaer (2019), the RFB-320 variant
  (`models/onnx/version-RFB-320.onnx`).
- **License**: MIT (permits redistribution, including in a commercial
  product, with the notice below retained).
- **Input/output contract** implemented in `../src/face.rs`, taken from that
  repo's own `detect_imgs_onnx.py` and `vision/utils/box_utils_numpy.py`
  reference scripts: input `input` is a `[1, 3, 240, 320]` RGB tensor
  normalized as `(pixel - 127) / 128`; outputs `scores` `[1, N, 2]`
  (background/face confidence per prior box) and `boxes` `[1, N, 4]`
  (corner-form, normalized to `[0, 1]`) are used directly (this model bakes
  anchor-box decoding into the graph, so no separate prior-box decoding step
  is needed) and combined via confidence thresholding + greedy IoU
  non-max-suppression.

```
MIT License

Copyright (c) 2019 linzai

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## pipnet_landmarks_300w_68.onnx

- **Source**: the ONNX export of PIPNet's `pipnet_r18_300w_celeba_68.pth`
  checkpoint (ResNet-18 backbone, 300W+CelebA semi-supervised/GSSL training,
  68 landmarks), as published by
  [yakhyo/pipnet-onnx](https://github.com/yakhyo/pipnet-onnx). The checkpoint
  itself originates from the original paper's implementation,
  [jhb86253817/PIPNet](https://github.com/jhb86253817/PIPNet) ("Pixel-in-Pixel
  Net: Towards Efficient Facial Landmark Detection in the Wild", Jin et al.).
  Both repositories are MIT licensed (full texts below), so the redistributed
  weights and the decode logic they're paired with are both clear for
  commercial use.
- **Input/output contract** implemented in `../src/landmarks.rs`, a direct
  translation of `yakhyo/pipnet-onnx`'s `model/pipnet_onnx.py` and
  `model/meanface.py`: input `input` is a `[1, 3, 256, 256]` RGB tensor,
  ImageNet-normalized, cropped from an already-detected face box with an
  asymmetric 10% pad (more on left/right/bottom, less on top); outputs
  `cls_map`, `offset_x`, `offset_y`, `nb_x`, `nb_y` (each `[1, 68, 8, 8]` or
  `[1, 680, 8, 8]` for the neighbor tensors) are decoded via PIPNet's
  "pixel-in-pixel" scheme - each landmark's peak location predicts its own
  offset and the offsets of its 10 nearest neighbors, and the final position
  averages a landmark's own prediction with every neighbor-prediction that
  targets it. The mean-face table used to build the neighbor index (68-point,
  300W layout) is vendored from upstream PIPNet's `data/meanface.txt` inside
  `landmarks.rs` itself. Landmark indices 36-41 and 42-47 are the two eyes in
  the standard iBUG/300W point order, used by `landmarks::eye_aspect_ratio`
  for the blink signal (`FrameMetrics::eyes_closed`).

```
MIT License

Copyright (c) 2020 Haibo Jin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

```
MIT License

Copyright (c) 2026 Yakhyokhuja Valikhujaev

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
