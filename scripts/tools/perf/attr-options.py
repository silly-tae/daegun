import json, sys

d = sys.argv[1]
o = json.load(open(f"{d}/base.json"))
o["CPU Counters"]["useDebuggingInformation"] = True
o["CPU Counters"]["useHighFrequencyForGuidedMode"] = True
o["CPU Counters"]["useHighFrequencyForManualMode"] = True
o["Time Profiler"]["highFrequencySampling"] = True
json.dump(o, open(f"{d}/opts.json", "w"))
