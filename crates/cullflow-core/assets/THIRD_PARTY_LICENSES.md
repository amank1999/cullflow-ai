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
