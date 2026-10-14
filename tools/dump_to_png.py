# 把 sophon-tui --dump-screen 导出的 JSON 渲染成 PNG（RGB 不透明）。
# 用法：python tools/dump_to_png.py <in.json> <out.png> [cell_w cell_h]
import json
import sys

from PIL import Image, ImageDraw, ImageFont


def load_font(size):
    for path in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\cour.ttf",
        r"C:\Windows\Fonts\consola.ttf",
    ]:
        try:
            return ImageFont.truetype(path, size)
        except Exception:
            continue
    return ImageFont.load_default()


def main():
    in_path = sys.argv[1]
    out_path = sys.argv[2]
    cw = int(sys.argv[3]) if len(sys.argv) > 3 else 9
    ch = int(sys.argv[4]) if len(sys.argv) > 4 else 18

    with open(in_path, "r", encoding="utf-8") as f:
        data = json.load(f)
    w = data["width"]
    h = data["height"]
    cells = data["cells"]

    img = Image.new("RGB", (w * cw, h * ch), (0, 0, 0))
    draw = ImageDraw.Draw(img)
    font = load_font(ch - 4)

    for y in range(h):
        for x in range(w):
            cell = cells[y][x]
            sym = cell.get("s", " ")
            fg = cell.get("fg")
            bg = cell.get("bg")
            bold = cell.get("b", False)
            fg_rgb = tuple(fg) if isinstance(fg, list) and len(fg) == 3 else (200, 200, 210)
            bg_rgb = tuple(bg) if isinstance(bg, list) and len(bg) == 3 else (17, 17, 27)
            # 背景（不透明）
            draw.rectangle(
                [x * cw, y * ch, (x + 1) * cw - 1, (y + 1) * ch - 1],
                fill=bg_rgb,
            )
            if sym and sym != " ":
                # 粗体：画两次轻微偏移
                pos = (x * cw + 1, y * ch + 1)
                draw.text(pos, sym, font=font, fill=fg_rgb)
                if bold:
                    draw.text((pos[0] + 1, pos[1]), sym, font=font, fill=fg_rgb)
    img.save(out_path, "PNG")
    print(f"saved {out_path} ({w * cw}x{h * ch})")


if __name__ == "__main__":
    main()
