#!/usr/bin/env python3
"""Capture macOS sample stacks and summarize only wire's guest runtime.

No guest opcode instrumentation. Inclusive hot functions may overlap; primary
categories partition the leaf weights, including recursive guest callbacks.
"""
import argparse
import collections
import html
import json
import os
import pathlib
import re
import statistics
import subprocess
import sys
import time


def compare(args):
    """Alternate unchanged IR inputs across two frozen interpreter builds."""
    args.out.mkdir(parents=True, exist_ok=True)
    runs = []
    for pair in range(args.pairs):
        order = ["before", "after"] if pair % 2 == 0 else ["after", "before"]
        for version in order:
            folder = args.out / f"pair-{pair}-{version}"
            folder.mkdir(exist_ok=True)
            command = [str(getattr(args, version).resolve()), "run", str(args.exe.resolve()),
                       "--ir", str(args.ir.resolve()), "--max-instr", str(args.max_instr),
                       "--log-dir", str(folder.resolve())]
            with (folder / "report.txt").open("w") as output:
                start = time.perf_counter()
                process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT)
                _, status, usage = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(status)
                wall = time.perf_counter() - start
            report = (folder / "report.txt").read_text()
            timing = re.search(r"instructions: (\d+) in ([\d.]+)s \(([\d.]+) Minstr/s\)", report)
            state = re.search(r"final regs:.*\n.*\n.*", report)
            outcome = re.search(r"^outcome: (.*)$", report, re.MULTILINE)
            apis = re.search(r"^api calls: (\d+)$", report, re.MULTILINE)
            if not all([timing, state, outcome, apis]):
                raise SystemExit(f"missing guest report in {folder}/report.txt")
            run = dict(pair=pair, version=version, process_cpu_s=usage.ru_utime + usage.ru_stime,
                       process_wall_s=wall, guest_wall_s=float(timing[2]),
                       guest_mips=float(timing[3]), instructions=int(timing[1]),
                       api_calls=int(apis[1]), state=state[0], outcome=outcome[1],
                       exit_code=process.returncode)
            runs.append(run)
            print(f"pair {pair} {version}: {run['guest_mips']:.2f} MIPS, "
                  f"process CPU {run['process_cpu_s']:.4f}s", flush=True)
    medians = {version: {key: statistics.median(r[key] for r in runs if r['version'] == version)
                         for key in ['guest_mips', 'process_cpu_s', 'process_wall_s']}
               for version in ['before', 'after']}
    summary = dict(commands=dict(before=str(args.before.resolve()), after=str(args.after.resolve()),
                                 exe=str(args.exe.resolve()), ir=str(args.ir.resolve()),
                                 max_instr=args.max_instr), medians=medians, runs=runs)
    (args.out / "compare.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(medians, indent=2))


def capture(args):
    if sys.platform != "darwin":
        raise SystemExit("capture requires macOS /usr/bin/sample")
    args.out.mkdir(parents=True, exist_ok=True)
    for run in range(args.runs):
        folder = args.out / f"run-{run}"
        folder.mkdir(exist_ok=True)
        command = [str(args.binary.resolve()), "run", str(args.exe.resolve()),
                   "--max-instr", str(args.max_instr), "--log-dir", str(folder.resolve())]
        if args.ir:
            command += ["--ir", str(args.ir.resolve())]
        with (folder / "report.txt").open("w") as report:
            process = subprocess.Popen(command, stdout=report, stderr=subprocess.STDOUT)
            result = subprocess.run(
                ["/usr/bin/sample", str(process.pid), "5", "1", "-mayDie", "-fullPaths",
                 "-file", str((folder / "sample.txt").resolve())],
                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
            (folder / "sampler.txt").write_text(result.stdout)
            status = process.wait(timeout=30)
        print(f"run-{run}: sampler={result.returncode}, guest={status}", flush=True)
        if result.returncode:
            raise SystemExit(result.stdout)


def parse_sample(path):
    graph = path.read_text().split("Call graph:\n", 1)[1].split("Total number in stack", 1)[0]
    nodes, parents = [], []
    for line in graph.splitlines():
        match = re.match(r"([\s+!:|]*)(\d+) (.*)", line)
        if not match:
            continue
        depth, weight = len(match[1]), int(match[2])
        while parents and nodes[parents[-1]]["depth"] >= depth:
            parents.pop()
        node = dict(depth=depth, weight=weight, symbol=match[3].split("  (in ", 1)[0],
                    parent=parents[-1] if parents else None, children=[])
        if parents:
            nodes[parents[-1]]["children"].append(len(nodes))
        nodes.append(node)
        parents.append(len(nodes) - 1)
    return nodes


def demangle(symbols):
    symbols = sorted(symbols)
    result = subprocess.run(["xcrun", "llvm-cxxfilt"], input="\n".join(symbols) + "\n",
                            text=True, capture_output=True, check=True)
    names = result.stdout.splitlines()
    if len(names) != len(symbols):
        raise ValueError("demangler changed the number of symbols")
    return dict(zip(symbols, names))


def summarize(args):
    parsed = [(path, parse_sample(path)) for path in args.samples]
    names = demangle({n["symbol"] for _, nodes in parsed for n in nodes})
    folded, reports = collections.Counter(), []
    for path, nodes in parsed:
        categories, inclusive = collections.Counter(), collections.Counter()
        for index, node in enumerate(nodes):
            weight = node["weight"] - sum(nodes[c]["weight"] for c in node["children"])
            if weight < 0:
                raise ValueError(f"invalid stack tree at {path}: {node['symbol']}")
            if not weight:
                continue
            stack = []
            while index is not None:
                stack.append(names[nodes[index]["symbol"]])
                index = nodes[index]["parent"]
            stack.reverse()
            entry = next((i for i, s in enumerate(stack)
                          if "wire::cpu" in s and s.endswith("::run_loop")), None)
            if entry is None:
                continue  # exclude loader, IR parse/bind, reports, teardown
            stack = stack[entry:]
            text = ";".join(stack)
            categories["runtime"] += weight
            # Rust demangling prints Cpu's inherent methods as <Cpu>::fpu_op,
            # dropping the source module. Include that ancestor's self time.
            if "wire::cpu::fpu" in text or "::fpu_op" in text:
                group = "x87 software execution"
            elif "::step_x86" in text:
                group = "other raw fallback"
            elif "::steps_cached" in text:
                group = "IR execution and cache"
            elif "wire::api" in text:
                group = "API outside guest execution"
            else:
                group = "outer dispatch and other"
            categories[group] += weight
            for key in ["log_multiply", "exp2_minus_one", "atanh_log2", "from_str_r",
                        "step_0f", "quad_divide_small", "PendingFlags", "GuestMem>::ptr"]:
                if key in text:
                    inclusive[key] += weight
            folded[tuple(["wire guest runtime"] + stack)] += weight
        reports.append(dict(file=str(path), samples=dict(categories), inclusive=dict(inclusive)))
        print(path.parent.name, dict(categories), dict(inclusive))
    total = sum(r["samples"].get("runtime", 0) for r in reports)
    if not total:
        raise SystemExit("no guest run_loop samples found")
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "summary.json").write_text(json.dumps(reports, indent=2) + "\n")
    (args.out / "runtime-folded.txt").write_text(
        "\n".join(f"{';'.join(stack)} {n}" for stack, n in sorted(folded.items())) + "\n")
    flamegraph(folded, total, args.out / "flame.svg")


def flamegraph(folded, total, output):
    root = dict(name="wire guest runtime", weight=0, children={})
    for stack, weight in folded.items():
        node = root
        node["weight"] += weight
        for name in stack[1:]:
            node = node["children"].setdefault(name, dict(name=name, weight=0, children={}))
            node["weight"] += weight
    rows = max(map(len, folded))
    height = 90 + rows * 22
    svg = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1400 {height}">',
           '<rect width="1400" height="100%" fill="#fafafa"/>',
           '<text x="20" y="27" font-family="sans-serif" font-size="20">'
           'wire CPU profile: guest runtime only</text>',
           f'<text x="20" y="50" font-family="sans-serif" font-size="12">'
           f'{total} leaf-weighted samples; hover for full symbols. '
           'Startup/AOT/loading excluded. Width is sample share.</text>']

    def draw(node, x, depth):
        width = 1360 * node["weight"] / total
        y = height - 30 - depth * 22
        name = node["name"]
        color = ("#ee8f51" if "fpu" in name or "apfloat" in name else
                 "#75b3df" if "cpu" in name else
                 "#90c9a0" if "wire::api" in name else "#cbc9db")
        tooltip = html.escape(f"{name}: {node['weight']} samples ({node['weight']/total:.2%})")
        svg.append(f'<g><title>{tooltip}</title><rect x="{x:.2f}" y="{y}" '
                   f'width="{max(0, width - .4):.2f}" height="21" fill="{color}"/></g>')
        short = re.sub(r"\[[0-9a-f]+\]", "", name)
        if width > 55:
            text = html.escape(short[:max(1, int(width / 6) - 2)])
            svg.append(f'<text x="{x+3:.2f}" y="{y+15}" font-family="monospace" '
                       f'font-size="11" pointer-events="none">{text}</text>')
        for child in sorted(node["children"].values(), key=lambda n: -n["weight"]):
            draw(child, x, depth + 1)
            x += 1360 * child["weight"] / total

    draw(root, 20, 0)
    svg.append("</svg>")
    output.write_text("\n".join(svg) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    collect = commands.add_parser("capture")
    collect.add_argument("exe", type=pathlib.Path)
    collect.add_argument("--binary", type=pathlib.Path, default=pathlib.Path("target/release/wire"))
    collect.add_argument("--ir", type=pathlib.Path)
    collect.add_argument("--out", type=pathlib.Path, required=True)
    collect.add_argument("--runs", type=int, default=4)
    collect.add_argument("--max-instr", type=int, default=50_000_000)
    report = commands.add_parser("summarize")
    report.add_argument("samples", type=pathlib.Path, nargs="+")
    report.add_argument("--out", type=pathlib.Path, required=True)
    bench = commands.add_parser("compare")
    bench.add_argument("exe", type=pathlib.Path)
    bench.add_argument("--before", type=pathlib.Path, required=True)
    bench.add_argument("--after", type=pathlib.Path, default=pathlib.Path("target/release/wire"))
    bench.add_argument("--ir", type=pathlib.Path, required=True)
    bench.add_argument("--out", type=pathlib.Path, required=True)
    bench.add_argument("--pairs", type=int, default=5)
    bench.add_argument("--max-instr", type=int, default=50_000_000)
    args = parser.parse_args()
    if args.command == "capture":
        capture(args)
    elif args.command == "summarize":
        summarize(args)
    else:
        if args.pairs < 1:
            parser.error("--pairs must be positive")
        compare(args)


if __name__ == "__main__":
    main()
