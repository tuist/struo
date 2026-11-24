def _llvm_coverage_adapter():
    return _native_coverage_process_support() + '''def llvm_observation(options, report, directory, process_complete=None):
    files = []
    complete = report.get('status') == 'passed' and len(report.get('cases', [])) == 1
    try:
        complete = complete and (single_native_process(directory) if process_complete is None else process_complete)
        profiles = sorted(glob.glob(directory + '/*.profraw'))
        if len(profiles) != 1:
            raise ValueError('missing or subprocess coverage profiles')
        profile = directory + '/merged.profdata'
        subprocess.run([options['profdata'], 'merge', '-sparse', *profiles, '-o', profile], check=True)
        exported = pathlib.Path(directory, 'coverage.json')
        with exported.open('wb') as stream:
            subprocess.run([options['cov'], 'export', options['binary'], '-instr-profile=' + profile], check=True, stdout=stream)
        if exported.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('coverage export exceeds observation limit')
        native = json.loads(exported.read_text())
        root = pathlib.Path.cwd().resolve()
        indexed = {}
        for data in native.get('data', []):
            for file in data.get('files', []):
                path = pathlib.Path(file['filename']).resolve()
                if path in indexed:
                    raise ValueError('duplicate coverage file')
                indexed[path] = file
        for source in options['sources']:
            absolute = pathlib.Path(source).resolve()
            path = absolute.relative_to(root).as_posix()
            native_file = indexed[absolute]
            lines = {}
            segments = native_file['segments']
            for index, segment in enumerate(segments):
                line, column, count, has_count, entry = segment[:5]
                gap = len(segment) > 5 and segment[5]
                if not has_count or gap:
                    continue
                if type(count) is not int or count < 0 or line < 1:
                    raise ValueError('invalid coverage counter')
                end = segments[index + 1] if index + 1 < len(segments) else segment
                last = end[0] - (1 if end[1] == 1 and end[0] > line else 0)
                if last - line > 100000:
                    raise ValueError('invalid coverage range')
                for number in range(line, last + 1):
                    lines[number] = max(lines.get(number, 0), count)
            if not lines:
                raise ValueError('empty instrumented source')
            numbers = sorted(lines)
            files.append({'path': path, 'line_numbers': numbers, 'execution_counts': [lines[line] for line in numbers]})
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('incomplete LLVM coverage: ' + str(error))
        complete = False
    report['coverage'] = {'tool': 'llvm', 'tool_version': options['version'], 'platform': platform.system().lower() + '/' + platform.machine(), 'partial': not complete, 'files': files}
    return report
'''
