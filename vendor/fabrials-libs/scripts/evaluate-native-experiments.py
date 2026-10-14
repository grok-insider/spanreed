import argparse
import json
import math
import statistics
from pathlib import Path


def quantile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def interval(values):
    state = 18391
    estimates = []
    for _ in range(10000):
        selected = []
        for _ in values:
            state = (state * 1664525 + 1013904223) & 0xffffffff
            selected.append(values[math.floor(state / 4294967296 * len(values))])
        estimates.append(statistics.median(selected))
    estimates.sort()
    return [estimates[math.floor(len(estimates) * .025)], estimates[math.ceil(len(estimates) * .975) - 1]]


def compare(baseline, candidate):
    assert len(baseline) == len(candidate) == 8
    absolute = [a - b for a, b in zip(baseline, candidate)]
    relative = [100 * (a - b) / a for a, b in zip(baseline, candidate)]
    return {"baselineByBlock": baseline, "candidateByBlock": candidate,
            "baselineMedian": statistics.median(baseline), "candidateMedian": statistics.median(candidate),
            "pairedReductionByBlockPercent": relative, "pairedMedianReductionPercent": statistics.median(relative),
            "pairedMedianReductionAbsolute": statistics.median(absolute), "pairedBootstrap95Percent": interval(relative)}


def events(root):
    manifest = json.loads((root / "manifest.json").read_text())
    receipt = json.loads((root / "receipt.json").read_text())
    rows = [json.loads(line) for line in (root / "samples.jsonl").read_text().splitlines()]
    assert len(rows) == 8000 and len(receipt["segments"]) == 32
    expected = [(channels, block, variant, sample)
                for channels in [1, 10]
                for block in range(8)
                for variant in (["notified", "polling"] if manifest["candidateFirst"][block] else ["polling", "notified"])
                for sample in range(250)]
    assert [(r["channels"], r["block"], r["variant"], r["sample"]) for r in rows] == expected
    for row in rows:
        assert row["warmup"] == (row["sample"] < 50)
        observed = row["observed"]
        assert len(observed) == row["channels"]
        assert [x["channel"] for x in observed] == list(range(row["channels"]))
        assert all(x["sequence"] == row["sample"] + 1 for x in observed)
        assert all(x["slow"] == (row["channels"] == 10 and x["channel"] == 9) for x in observed)
        assert all(math.isfinite(x["latencyMs"]) and x["latencyMs"] >= 0 for x in observed)
    conditions = []
    for channels in [1, 10]:
        def percentile(variant, block, slow=None):
            return quantile([max(x["latencyMs"] for x in r["observed"] if slow is None or x["slow"] == slow) for r in rows if not r["warmup"] and r["channels"] == channels and r["block"] == block and r["variant"] == variant], .95)
        latency = compare([percentile("polling", b) for b in range(8)], [percentile("notified", b) for b in range(8)])
        memory = compare([max(r["resources"]["rssKiB"] for r in rows if not r["warmup"] and r["channels"] == channels and r["block"] == b and r["variant"] == "polling") for b in range(8)], [max(r["resources"]["rssKiB"] for r in rows if not r["warmup"] and r["channels"] == channels and r["block"] == b and r["variant"] == "notified") for b in range(8)])
        def cpu_ticks(variant, block):
            segment = next(s for s in receipt["segments"] if s["channels"] == channels and s["block"] == block and s["variant"] == variant)
            return sum(segment["resourcesAfter"][k] - segment["resourcesBefore"][k] for k in ["userTicks", "systemTicks"])
        cpu = compare([cpu_ticks("polling", b) for b in range(8)], [cpu_ticks("notified", b) for b in range(8)])
        gates = {"cpuPerCompletedBatch": cpu["pairedMedianReductionPercent"] >= -10,
                 "latencyPercent": latency["pairedMedianReductionPercent"] >= 20,
                 "latencyAbsolute": latency["pairedMedianReductionAbsolute"] >= 20,
                 "uncertainty": latency["pairedBootstrap95Percent"][0] > 0,
                 "memory": memory["pairedMedianReductionPercent"] >= -10}
        result = {"channels": channels, "allConsumerP95Ms": latency, "peakMeasuredRssKiB": memory, "cpuTicksPer250Samples": cpu, "gates": gates}
        if channels == 10:
            result["fastConsumersP95Ms"] = compare([percentile("polling", b, False) for b in range(8)], [percentile("notified", b, False) for b in range(8)])
            result["slowConsumerP95Ms"] = compare([percentile("polling", b, True) for b in range(8)], [percentile("notified", b, True) for b in range(8)])
        conditions.append(result)
    return {"scope": manifest["scope"], "rowsVerified": len(rows), "deliveryObservationsVerified": sum(len(r["observed"]) for r in rows), "measuredDeliveryObservations": sum(len(r["observed"]) for r in rows if not r["warmup"]), "conditions": conditions, "nativeMetricGatesPassed": all(all(c["gates"].values()) for c in conditions), "fullChainQualified": False}


def publication(root):
    manifest = json.loads((root / "manifest.json").read_text())
    receipt = json.loads((root / "receipt.json").read_text())
    rows = [json.loads(line) for line in (root / "samples.jsonl").read_text().splitlines()]
    assert len(rows) == 4000 and len(receipt["segments"]) == 16
    assert [(r["block"], r["variant"], r["sample"]) for r in rows] == [(block, variant, sample) for block in range(8) for variant in (["contentdigest", "fullexport"] if manifest["candidateFirst"][block] else ["fullexport", "contentdigest"]) for sample in range(250)]
    assert all(r["warmup"] == (r["sample"] < 50) for r in rows)
    def segments(variant):
        return sorted([segment for segment in receipt["segments"] if segment["variant"] == variant], key=lambda value: value["block"])
    base, candidate = segments("fullexport"), segments("contentdigest")
    for segment in receipt["segments"]:
        selected = [r for r in rows if not r["warmup"] and r["block"] == segment["block"] and r["variant"] == segment["variant"]]
        assert sum(r["cpuTicks"] for r in selected) == segment["measuredCpuTicks"]
        assert all(segment[field] for field in ["unchangedRevisions", "documentIdentity"])
    def cpu_p95(variant, block):
        return quantile([r["cpuNanoseconds"] / 1e6 for r in rows if not r["warmup"] and r["variant"] == variant and r["block"] == block], .95)
    cpu = compare([cpu_p95("fullexport", block) for block in range(8)], [cpu_p95("contentdigest", block) for block in range(8)])
    memory = compare([max(s["rssKiB"]) for s in base], [max(s["rssKiB"]) for s in candidate])
    latency = compare([quantile(s["elapsedMs"], .95) for s in base], [quantile(s["elapsedMs"], .95) for s in candidate])
    uncached = [b["uncachedDocumentMs"] - a["uncachedDocumentMs"] for a, b in zip(base, candidate)]
    gates = {"cpu": cpu["pairedMedianReductionPercent"] >= 30,
             "uncertainty": cpu["pairedBootstrap95Percent"][0] > 0,
             "memory": memory["pairedMedianReductionPercent"] >= -10,
             "uncachedDocument": max(uncached) <= 250}
    return {"scope": manifest["scope"], "rowsVerified": len(rows), "cpuP95MsPerBatch": cpu, "peakMeasuredRssKiB": memory, "batchP95Ms": latency, "uncachedDocumentRegressionMsByBlock": uncached, "gates": gates, "nativeMetricGatesPassed": all(gates.values()), "publicPublishingQualified": False, "bytesReadReduction": 0}


parser = argparse.ArgumentParser()
parser.add_argument("kind", choices=["events", "publication"])
parser.add_argument("evidence", type=Path)
parser.add_argument("--historical", action="store_true")
args = parser.parse_args()
result = (events if args.kind == "events" else publication)(args.evidence)
result["historicalOnly"] = args.historical
result["decision"] = "historical_not_release_evidence" if args.historical else "inconclusive_resources_and_end_to_end_pending" if result["nativeMetricGatesPassed"] else "revise_experiment"
result["method"] = {"quantile": "nearest rank", "pairedBootstrapReplicates": 10000, "bootstrapSeed": 18391, "statistic": "median of paired block improvements"}
result["resourceQualification"] = {"status": "pending", "missing": ["cpu-idle-percent", "wakeups-per-second"], "present": ["resident-bytes", "cpu-during-work"]}
print(json.dumps(result, indent=2))
