"""
从各平台的 make_dryrun.log 中提取实际需要的 C 文件和头文件，以减少最终大小。
最终将 ffmpeg_slim/ 和 configs/ 分别打包为 ffmpeg_slim.zip 和 configs.zip。
"""

import os
import re
import shutil
import zipfile
from pathlib import Path


def parse_log(log_path: Path):
    """解析一个 make_dryrun.log，返回 (c_files, include_dirs)，路径均相对于 ffmpeg/ 目录"""
    c_files: set[str] = set()
    include_dirs: set[str] = set()

    with open(log_path, encoding="utf-8", errors="ignore") as f:
        for line in f:
            if ("-c -o " not in line and "-c -Fo" not in line) or ".c" not in line:
                continue
            parts = line.split()
            for part in parts:
                # 提取 -I 头文件搜索路径
                if part.startswith("-I"):
                    inc = part[2:]
                    if inc in (".", ""):
                        continue
                    # 找到路径中 ffmpeg/ 之后的部分
                    for sep in ("ffmpeg/", "ffmpeg\\"):
                        idx = inc.find(sep)
                        if idx != -1:
                            rel = inc[idx + len(sep):]
                            if rel:
                                include_dirs.add(rel.replace("\\", "/"))
                            break
                # 提取 .c 源文件路径
                elif part.endswith(".c"):
                    for marker in ("libav", "libsw", "compat/"):
                        idx = part.find(marker)
                        if idx != -1:
                            c_files.add(part[idx:].replace("\\", "/"))
                            break

    return c_files, include_dirs


def copy_file(src: Path, dst: Path):
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)


def main():
    ffmpeg_src = Path("ffmpeg")
    slim_dst = Path("ffmpeg_slim")
    configs_dir = Path("configs")

    if not ffmpeg_src.exists():
        print("错误：ffmpeg/ 目录不存在")
        return

    if slim_dst.exists():
        print(f"清理旧的 {slim_dst}/ ...")
        shutil.rmtree(slim_dst)

    # ── 1. 解析所有平台日志 ──────────────────────────────────────────────
    all_c_files: set[str] = set()
    all_include_dirs: set[str] = set()

    for platform_dir in sorted(configs_dir.iterdir()):
        if not platform_dir.is_dir():
            continue
        log_path = platform_dir / "make_dryrun.log"
        if not log_path.exists():
            continue
        c_files, include_dirs = parse_log(log_path)
        all_c_files.update(c_files)
        all_include_dirs.update(include_dirs)
        print(f"  {platform_dir.name}: {len(c_files)} C 文件, {len(include_dirs)} -I 路径")

    print(f"\n合并后：{len(all_c_files)} 个唯一 C 文件，{len(all_include_dirs)} 个唯一 -I 路径")
    print("额外 -I 路径（compat 等）：")
    for d in sorted(all_include_dirs):
        print(f"  {d}")

    # ── 2. 复制 C 源文件 ─────────────────────────────────────────────────
    print("\n复制 C 源文件...")
    c_ok = c_miss = 0

    def copy_c_file(rel_str: str) -> Path | None:
        """复制一个 C 文件到 ffmpeg_slim，返回目标路径，文件不存在返回 None"""
        raw = ffmpeg_src / rel_str
        resolved = Path(os.path.normpath(raw))
        try:
            rel_to_src = resolved.relative_to(ffmpeg_src)
        except ValueError:
            rel_to_src = Path(os.path.normpath(rel_str))
        dst = slim_dst / rel_to_src
        if resolved.exists():
            copy_file(resolved, dst)
            return dst
        return None

    for rel_str in sorted(all_c_files):
        dst = copy_c_file(rel_str)
        if dst:
            c_ok += 1
        else:
            print(f"  !! 缺失: {ffmpeg_src / rel_str}")
            c_miss += 1

    print(f"  成功: {c_ok}  缺失: {c_miss}")

    # ── 2b. 递归处理 #include "*.c" 模式 ─────────────────────────────────
    # FFmpeg 中大量使用模板文件（如 resample_template.c），这些文件通过
    # #include 被其他 C 文件引入，不会出现在 make_dryrun.log 里，需要额外复制
    print("\n查找并复制被 #include 的 .c 模板文件...")
    include_c_pattern = re.compile(r'#include\s+"([^"]+\.c)"')
    extra_copied = 0
    rounds = 0

    while True:
        rounds += 1
        newly_needed: set[str] = set()

        for c_file in list(slim_dst.rglob("*.c")):
            try:
                content = c_file.read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            for m in include_c_pattern.finditer(content):
                included = m.group(1)
                # included 路径相对于当前 c_file 所在目录，或者是带路径的相对路径
                candidate = (c_file.parent / included).resolve()
                # 映射回 ffmpeg_src 的相对路径
                try:
                    rel = candidate.relative_to(slim_dst.resolve())
                    src_candidate = ffmpeg_src / rel
                    dst_candidate = slim_dst / rel
                    if not dst_candidate.exists() and src_candidate.exists():
                        newly_needed.add(str(rel).replace("\\", "/"))
                except ValueError:
                    pass

        if not newly_needed:
            print(f"  共经过 {rounds} 轮扫描，无新增依赖")
            break

        for rel_str in sorted(newly_needed):
            dst = copy_c_file(rel_str)
            if dst:
                extra_copied += 1
            else:
                print(f"  !! 被 include 的 C 文件缺失: {ffmpeg_src / rel_str}")

    print(f"  额外复制了 {extra_copied} 个被 #include 的 C 模板文件")

    # ── 3. 复制头文件 ────────────────────────────────────────────────────
    # 收集所有需要扫描的目录（相对于 ffmpeg/）
    h_scan_roots: set[str] = set()
    # 标准 lib 目录（总是需要）
    for lib in ("libavcodec", "libavformat", "libavutil", "libswresample"):
        h_scan_roots.add(lib)
    # compat 全树（包含 atomics/win32、stdbit 等）
    h_scan_roots.add("compat")
    # 日志中出现的额外 -I 目录
    for inc_rel in all_include_dirs:
        # 取顶层目录名作为扫描根（避免重复扫描）
        top = inc_rel.split("/")[0]
        h_scan_roots.add(top)
    # ffmpeg/ 根目录下可能有少量 .h（如 libcompat）
    h_scan_roots.add("")

    print("\n复制头文件...")
    h_ok = 0
    scanned: set[str] = set()

    for root_rel in sorted(h_scan_roots):
        src_dir = ffmpeg_src / root_rel if root_rel else ffmpeg_src
        if not src_dir.exists() or root_rel in scanned:
            continue
        scanned.add(root_rel)

        for h_file in src_dir.rglob("*.h"):
            try:
                rel = h_file.relative_to(ffmpeg_src)
            except ValueError:
                continue
            dst = slim_dst / rel
            copy_file(h_file, dst)
            h_ok += 1

    print(f"  成功: {h_ok} 个头文件")

    # ── 4. 汇总 ─────────────────────────────────────────────────────────
    all_slim = list(slim_dst.rglob("*"))
    total_size = sum(f.stat().st_size for f in all_slim if f.is_file()) / 1_048_576
    print(f"\nffmpeg_slim/ 生成完毕")
    print(f"   文件总数: {sum(1 for f in all_slim if f.is_file())}")
    print(f"   磁盘占用: {total_size:.1f} MB")

    # ── 5. 打包 vendor/ffmpeg_slim.zip ──────────────────────────────────────
    # 条目路径格式：libavcodec/foo.c（不含 ffmpeg_slim/ 前缀）
    # 解压到 OUT_DIR/ffmpeg_slim/ 后得到 OUT_DIR/ffmpeg_slim/libavcodec/foo.c
    Path("crates/ffmpeg_audio_sys/vendor").mkdir(exist_ok=True)
    slim_zip = Path("crates/ffmpeg_audio_sys/vendor/ffmpeg_slim.zip")
    print(f"\n打包 {slim_zip} ...")
    with zipfile.ZipFile(slim_zip, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for file in sorted(slim_dst.rglob("*")):
            if file.is_file():
                arcname = file.relative_to(slim_dst).as_posix()
                zf.write(file, arcname)
    slim_zip_size = slim_zip.stat().st_size / 1_048_576
    print(f"  {slim_zip}：{slim_zip_size:.1f} MB")

    # ── 6. 打包 vendor/configs.zip ──────────────────────────────────────────
    # 条目路径格式：build_out_xxx/config.h（不含 configs/ 前缀）
    # 解压到 OUT_DIR/configs/ 后得到 OUT_DIR/configs/build_out_xxx/config.h
    configs_zip = Path("crates/ffmpeg_audio_sys/vendor/configs.zip")
    print(f"\n打包 {configs_zip} ...")
    with zipfile.ZipFile(configs_zip, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for file in sorted(configs_dir.rglob("*")):
            if file.is_file():
                arcname = file.relative_to(configs_dir).as_posix()
                zf.write(file, arcname)
    configs_zip_size = configs_zip.stat().st_size / 1_048_576
    print(f"  {configs_zip}：{configs_zip_size:.1f} MB")

    print("\n✅ 完成！ffmpeg_slim.zip 和 configs.zip 已生成")


if __name__ == "__main__":
    main()
