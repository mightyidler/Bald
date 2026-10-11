"""Summarize scoped probe samples, excluding the first five seconds per phase."""
import csv
import json
import statistics
import sys
from pathlib import Path


def summarize(executable):
    prefix = Path(executable)
    with prefix.with_suffix('.csv').open(encoding='utf-8-sig', newline='') as source:
        rows = list(csv.DictReader(source))
    events = [json.loads(line) for line in prefix.with_suffix('.events.jsonl').read_text(encoding='utf-8').splitlines()]
    number = lambda row, key: float(row[key] or 0)
    result = {}
    phases = list(dict.fromkeys(row['Phase'] for row in rows))
    for phase in phases:
        all_samples = [row for row in rows if row['Phase'] == phase]
        start = number(all_samples[0], 'Seconds')
        samples = [row for row in all_samples if number(row, 'Seconds') >= start + 5]
        if len(samples) < 2:
            continue
        first, last = samples[0], samples[-1]
        duration = number(last, 'Seconds') - number(first, 'Seconds')
        median = lambda key: statistics.median(number(row, key) for row in samples)
        values = {key + 'MiB': round(median(key) / 2**20, 2) for key in ['MainWorkingSet', 'MainPrivate', 'WebWorkingSet', 'WebPrivate']}
        values['TotalWorkingSetMiB'] = round(statistics.median(number(row, 'MainWorkingSet') + number(row, 'WebWorkingSet') for row in samples) / 2**20, 2)
        for kind in ['Main', 'Web']:
            values[kind + 'CpuOneCorePercent'] = round((number(last, kind + 'CpuMs') - number(first, kind + 'CpuMs')) / duration / 10, 3)
        for key in ['WebProcesses', 'MainThreads', 'WebThreads', 'MainHandles', 'WebHandles']:
            values[key] = median(key)
        visible_events = [event for event in events if number(first, 'Seconds') <= event['seconds'] <= number(last, 'Seconds') and event['kind'] == 'state']
        state = [json.loads(event['data']) for event in visible_events]
        values['StateRequestsPerSecond'] = round(len(state) / duration, 3)
        values['StateBytesPerSecond'] = round(sum(item['bytes'] for item in state) / duration)
        if state:
            values['StateResponseBytesMedian'] = statistics.median(item['bytes'] for item in state)
            values['StateLatencyMsMedian'] = round(statistics.median(item['ms'] for item in state), 2)
        result[phase] = values
    result['actions'] = [{key: event[key] for key in ['seconds', 'kind', 'data']} for event in events if event['kind'] in ['phase', 'native-action-ms', 'picker', 'counts', 'error', 'js-error', 'reported-error']]
    intervals = []
    phase = None
    previous = None
    for event in events:
        if event['kind'] == 'phase':
            phase = event['data']
        elif event['kind'] == 'counts':
            counts = json.loads(event['data'])
            if previous is not None:
                elapsed = event['seconds'] - previous[0]
                intervals.append({
                    'phase': previous[1],
                    'seconds': round(elapsed, 3),
                    **{key + 'PerSecond': round((value - previous[2][key]) / elapsed, 3)
                       for key, value in counts.items()},
                })
            previous = (event['seconds'], phase, counts)
    result['nativeIntervals'] = intervals
    return result


if __name__ == '__main__':
    print(json.dumps({argument: summarize(argument) for argument in sys.argv[1:]}, indent=2, ensure_ascii=False))
