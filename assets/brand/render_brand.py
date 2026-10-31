"""Render LockIn SVG sources to application PNG/ICO assets and a review sheet."""

from __future__ import annotations

from io import BytesIO
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
import resvg_py


ROOT = Path(__file__).resolve().parent
PNG_DIR = ROOT / "png"
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)


def raster(svg: Path, size: int) -> Image.Image:
    data = resvg_py.svg_to_bytes(svg.read_text(encoding="utf-8"), width=size, height=size)
    return Image.open(BytesIO(data)).convert("RGBA")


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    family = "segoeuib.ttf" if bold else "segoeui.ttf"
    path = Path("C:/Windows/Fonts") / family
    return ImageFont.truetype(path, size) if path.exists() else ImageFont.load_default()


def contain(image: Image.Image, box: tuple[int, int, int, int]) -> tuple[int, int]:
    x0, y0, x1, y1 = box
    copy = image.copy()
    copy.thumbnail((x1 - x0, y1 - y0), Image.Resampling.LANCZOS)
    return x0 + (x1 - x0 - copy.width) // 2, y0 + (y1 - y0 - copy.height) // 2


def render_app_assets() -> list[Image.Image]:
    PNG_DIR.mkdir(parents=True, exist_ok=True)
    frames: list[Image.Image] = []
    for size in SIZES:
        source = ROOT / ("app-icon-small.svg" if size <= 24 else "app-icon.svg")
        image = raster(source, size)
        image.save(PNG_DIR / f"app-icon-{size}.png", optimize=True)
        frames.append(image)
    frames[-1].save(
        ROOT / "lockin.ico",
        format="ICO",
        append_images=frames[:-1],
        sizes=[(size, size) for size in SIZES],
    )
    return frames


def render_preview(frames: list[Image.Image]) -> None:
    bg, surface, raised = "#14171D", "#1B1F27", "#232833"
    text, muted, line = "#E4E7EC", "#8A93A3", "#2E3440"
    sheet = Image.new("RGB", (1600, 1000), bg)
    draw = ImageDraw.Draw(sheet)

    draw.text((72, 54), "LOCKIN / IDENTITY STUDY", fill=muted, font=font(18, True))
    draw.text((72, 86), "Calm attention, made visible.", fill=text, font=font(42, True))
    draw.text((72, 145), "Three reduced marks · selected system · Windows size test", fill=muted, font=font(20))

    concepts = [ROOT / "concepts" / f"concept-{i}.svg" for i in range(1, 4)]
    names = ["01  FOCUS LATCH", "02  GAZE / PAGE", "03  CALIBRATION GATE"]
    notes = ["SELECTED", "STUDY-SPECIFIC", "SYSTEM / BOUNDARY"]
    for index, (source, name, note) in enumerate(zip(concepts, names, notes)):
        x = 72 + index * 360
        draw.rounded_rectangle((x, 215, x + 328, 525), radius=24, fill=surface, outline=line, width=2)
        icon = raster(source, 164)
        sheet.paste(icon, (x + 82, 258), icon)
        draw.text((x + 24, 458), name, fill=text, font=font(16, True))
        draw.text((x + 24, 486), note, fill="#8CC7A5" if index == 0 else muted, font=font(13, True))

    x0 = 1152
    draw.rounded_rectangle((x0, 215, 1528, 525), radius=24, fill=raised, outline=line, width=2)
    app = raster(ROOT / "app-icon.svg", 224)
    sheet.paste(app, (x0 + 76, 246), app)
    draw.text((x0 + 24, 486), "FINAL APPLICATION ICON", fill=text, font=font(16, True))

    draw.rounded_rectangle((72, 570, 1000, 918), radius=24, fill=surface, outline=line, width=2)
    draw.text((102, 602), "PRIMARY SIGNATURE", fill=muted, font=font(14, True))
    wordmark = resvg_py.svg_to_bytes((ROOT / "wordmark.svg").read_text(encoding="utf-8"), width=768, height=192)
    wm = Image.open(BytesIO(wordmark)).convert("RGBA")
    sheet.paste(wm, (132, 650), wm)
    draw.text((102, 862), "Sage carries focus · periwinkle carries gaze · soft white anchors intent", fill=muted, font=font(17))

    draw.rounded_rectangle((1032, 570, 1528, 918), radius=24, fill=surface, outline=line, width=2)
    draw.text((1062, 602), "TASKBAR LEGIBILITY", fill=muted, font=font(14, True))
    scale = 4
    cursor_x = 1062
    for image, size in zip(frames[:7], SIZES[:7]):
        display = image.resize((size * scale, size * scale), Image.Resampling.NEAREST)
        if cursor_x + display.width > 1498:
            break
        y = 690 - display.height // 2
        sheet.paste(display, (cursor_x, y), display)
        label_w = draw.textbbox((0, 0), str(size), font=font(13))[2]
        draw.text((cursor_x + (display.width - label_w) // 2, 772), str(size), fill=muted, font=font(13))
        cursor_x += display.width + 28
    draw.text((1062, 835), "16 / 20 / 24 use the simplified cut", fill=text, font=font(16, True))
    draw.text((1062, 864), "Heavier ring, border removed.", fill=muted, font=font(15))

    sheet.save(ROOT / "preview.png", optimize=True)


if __name__ == "__main__":
    render_preview(render_app_assets())
