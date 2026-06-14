"""
EcoAlert MATLAB Algorithm Visualizer
Runs MATLAB detector on video and displays results in an interactive HTML dashboard
"""

import subprocess
import json
import os
import sys
from pathlib import Path
import csv
from datetime import datetime


def run_matlab_analysis(video_path: str, person_threshold: float = 0.65,
                       light_threshold: float = 0.70, frame_step: int = 10):
    """
    Run MATLAB analysis script and export results to CSV
    """
    # Get the directory of this script
    script_dir = Path(__file__).parent

    # Create a temporary MATLAB script that exports to CSV
    matlab_script = f"""
% Auto-generated visualization runner
clear; clc;

videoPath = "{video_path.replace(chr(92), chr(92)*2)}";
frameStep = {frame_step};
personThreshold = {person_threshold};
lightThreshold = {light_threshold};

detector = EcoAlertDetector(personThreshold, lightThreshold);
roiConfig = EcoAlertDetector.defaultRoiConfig();

if ~exist(videoPath, 'file')
    error('Video file not found: %s', videoPath);
end

reader = VideoReader(videoPath);
rows = {{}};
frameIndex = 0;
analyzed = 0;

fprintf('Analyzing video: %s\\n', videoPath);
fprintf('Resolution: %dx%d, FPS: %.2f\\n\\n', reader.Width, reader.Height, reader.FrameRate);

while hasFrame(reader)
    frame = readFrame(reader);
    frameIndex = frameIndex + 1;

    if mod(frameIndex - 1, frameStep) ~= 0
        continue;
    end

    result = detector.analyzeScene(frame, roiConfig, "ColorOrder", "rgb");
    scene = result.scene;
    analyzed = analyzed + 1;

    rows(end + 1, :) = {{ ...
        frameIndex, reader.CurrentTime, scene.person, scene.light, ...
        scene.person_confidence, scene.light_confidence, ...
        scene.light_brightness, scene.color_score, scene.motion_score, ...
        scene.reason}}; %#ok<SAGROW>

    if mod(analyzed, 50) == 0
        fprintf('Processed %d frames...\\n', analyzed);
    end
end

% Export to CSV
outputFile = "{script_dir / 'analysis_results.csv'}";
resultTable = cell2table(rows, "VariableNames", {{ ...
    "frame", "time_sec", "person", "light", "person_confidence", ...
    "light_confidence", "light_brightness", "color_score", ...
    "motion_score", "reason"}});

writetable(resultTable, outputFile);
fprintf('\\nAnalysis complete! Results saved to: %s\\n', outputFile);
fprintf('Total frames analyzed: %d\\n', analyzed);
"""

    matlab_script_path = script_dir / "temp_analysis_runner.m"
    with open(matlab_script_path, 'w') as f:
        f.write(matlab_script)

    print("Running MATLAB analysis...")
    print(f"Video: {video_path}")
    print(f"Frame step: {frame_step}")
    print("-" * 60)

    try:
        # Run MATLAB script
        result = subprocess.run(
            ['matlab', '-batch', f'run("{matlab_script_path}")'],
            capture_output=True,
            text=True,
            timeout=3600  # 1 hour timeout
        )

        if result.returncode != 0:
            print("MATLAB execution failed:")
            print(result.stderr)
            return False

        print("MATLAB analysis completed successfully!")
        return True

    except FileNotFoundError:
        print("Error: MATLAB not found in PATH")
        print("Please ensure MATLAB is installed and accessible from command line")
        return False
    except subprocess.TimeoutExpired:
        print("Error: Analysis timed out (1 hour limit)")
        return False
    finally:
        # Clean up temp script
        if matlab_script_path.exists():
            matlab_script_path.unlink()


def generate_html_dashboard(csv_path: str, output_path: str):
    """
    Generate an interactive HTML dashboard from analysis results
    """
    # Read CSV data
    data = []
    with open(csv_path, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            data.append({
                'frame': int(row['frame']),
                'time_sec': float(row['time_sec']),
                'person': int(row['person']),
                'light': int(row['light']),
                'person_confidence': float(row['person_confidence']),
                'light_confidence': float(row['light_confidence']),
                'light_brightness': float(row['light_brightness']),
                'color_score': float(row['color_score']),
                'motion_score': float(row['motion_score']),
                'reason': row['reason']
            })

    if not data:
        print("No data found in CSV file")
        return False

    # Calculate statistics
    total_frames = len(data)
    person_detected_count = sum(1 for d in data if d['person'] == 1)
    light_on_count = sum(1 for d in data if d['light'] == 1)
    avg_brightness = sum(d['light_brightness'] for d in data) / total_frames
    avg_motion = sum(d['motion_score'] for d in data) / total_frames
    avg_color = sum(d['color_score'] for d in data) / total_frames

    # Convert data to JSON for JavaScript
    data_json = json.dumps(data)

    html_content = f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>EcoAlert MATLAB Analysis Dashboard</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.min.js"></script>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}

        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
            padding: 20px;
        }}

        .container {{
            max-width: 1600px;
            margin: 0 auto;
        }}

        h1 {{
            color: white;
            text-align: center;
            margin-bottom: 30px;
            font-size: 2.5em;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.3);
        }}

        .stats-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}

        .stat-card {{
            background: white;
            border-radius: 15px;
            padding: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.2);
            transition: transform 0.3s ease;
        }}

        .stat-card:hover {{
            transform: translateY(-5px);
        }}

        .stat-label {{
            color: #666;
            font-size: 0.9em;
            margin-bottom: 10px;
        }}

        .stat-value {{
            font-size: 2em;
            font-weight: bold;
            color: #333;
        }}

        .stat-value.person {{ color: #4CAF50; }}
        .stat-value.light {{ color: #FFC107; }}
        .stat-value.brightness {{ color: #2196F3; }}
        .stat-value.motion {{ color: #FF5722; }}

        .charts-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(500px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}

        .chart-container {{
            background: white;
            border-radius: 15px;
            padding: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.2);
        }}

        .chart-title {{
            font-size: 1.3em;
            font-weight: bold;
            margin-bottom: 15px;
            color: #333;
        }}

        .timeline-container {{
            background: white;
            border-radius: 15px;
            padding: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.2);
            overflow-x: auto;
        }}

        .timeline {{
            display: flex;
            gap: 2px;
            min-width: fit-content;
        }}

        .timeline-frame {{
            width: 3px;
            height: 60px;
            cursor: pointer;
            transition: transform 0.2s;
            position: relative;
        }}

        .timeline-frame:hover {{
            transform: scaleY(1.2);
        }}

        .timeline-frame.person {{
            background: #4CAF50;
        }}

        .timeline-frame.no-person {{
            background: #e0e0e0;
        }}

        .tooltip {{
            position: fixed;
            background: rgba(0, 0, 0, 0.9);
            color: white;
            padding: 10px;
            border-radius: 5px;
            font-size: 0.85em;
            pointer-events: none;
            z-index: 1000;
            display: none;
            max-width: 300px;
        }}

        .controls {{
            background: white;
            border-radius: 15px;
            padding: 20px;
            margin-bottom: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.2);
        }}

        .control-group {{
            display: inline-block;
            margin-right: 20px;
            margin-bottom: 10px;
        }}

        .control-group label {{
            display: block;
            margin-bottom: 5px;
            color: #666;
            font-size: 0.9em;
        }}

        input[type="range"] {{
            width: 200px;
        }}

        button {{
            background: #667eea;
            color: white;
            border: none;
            padding: 10px 20px;
            border-radius: 5px;
            cursor: pointer;
            font-size: 1em;
            transition: background 0.3s;
        }}

        button:hover {{
            background: #764ba2;
        }}

        canvas {{
            max-height: 300px;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>EcoAlert MATLAB Analysis Dashboard</h1>

        <div class="stats-grid">
            <div class="stat-card">
                <div class="stat-label">Total Frames Analyzed</div>
                <div class="stat-value">{total_frames}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Person Detected</div>
                <div class="stat-value person">{person_detected_count} ({person_detected_count/total_frames*100:.1f}%)</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Light ON</div>
                <div class="stat-value light">{light_on_count} ({light_on_count/total_frames*100:.1f}%)</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Avg Brightness</div>
                <div class="stat-value brightness">{avg_brightness:.1f}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Avg Motion Score</div>
                <div class="stat-value motion">{avg_motion:.4f}</div>
            </div>
            <div class="stat-card">
                <div class="stat-label">Avg Color Score</div>
                <div class="stat-value">{avg_color:.4f}</div>
            </div>
        </div>

        <div class="controls">
            <div class="control-group">
                <label>Frame Step Filter: <span id="stepValue">1</span></label>
                <input type="range" id="frameStep" min="1" max="50" value="1">
            </div>
            <div class="control-group">
                <label>Show Only Person Detection:</label>
                <input type="checkbox" id="personOnly">
            </div>
            <button onclick="resetFilters()">Reset Filters</button>
        </div>

        <div class="charts-grid">
            <div class="chart-container">
                <div class="chart-title">Brightness Over Time</div>
                <canvas id="brightnessChart"></canvas>
            </div>
            <div class="chart-container">
                <div class="chart-title">Motion Score Over Time</div>
                <canvas id="motionChart"></canvas>
            </div>
            <div class="chart-container">
                <div class="chart-title">Color Score Over Time</div>
                <canvas id="colorChart"></canvas>
            </div>
            <div class="chart-container">
                <div class="chart-title">Detection Confidence</div>
                <canvas id="confidenceChart"></canvas>
            </div>
        </div>

        <div class="timeline-container">
            <div class="chart-title">Detection Timeline (Green = Person Detected)</div>
            <div class="timeline" id="timeline"></div>
        </div>
    </div>

    <div class="tooltip" id="tooltip"></div>

    <script>
        const allData = {data_json};
        let filteredData = [...allData];

        // Chart configuration
        const commonOptions = {{
            responsive: true,
            maintainAspectRatio: true,
            interaction: {{
                mode: 'index',
                intersect: false
            }},
            plugins: {{
                legend: {{
                    display: true
                }}
            }},
            scales: {{
                x: {{
                    title: {{
                        display: true,
                        text: 'Frame Number'
                    }}
                }},
                y: {{
                    beginAtZero: false
                }}
            }}
        }};

        let brightnessChart, motionChart, colorChart, confidenceChart;

        function initCharts() {{
            const labels = filteredData.map(d => d.frame);

            // Brightness Chart
            const briCtx = document.getElementById('brightnessChart').getContext('2d');
            brightnessChart = new Chart(briCtx, {{
                type: 'line',
                data: {{
                    labels: labels,
                    datasets: [{{
                        label: 'Brightness',
                        data: filteredData.map(d => d.light_brightness),
                        borderColor: '#2196F3',
                        backgroundColor: 'rgba(33, 150, 243, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4
                    }}]
                }},
                options: commonOptions
            }});

            // Motion Chart
            const motCtx = document.getElementById('motionChart').getContext('2d');
            motionChart = new Chart(motCtx, {{
                type: 'line',
                data: {{
                    labels: labels,
                    datasets: [{{
                        label: 'Motion Score',
                        data: filteredData.map(d => d.motion_score),
                        borderColor: '#FF5722',
                        backgroundColor: 'rgba(255, 87, 34, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4
                    }}, {{
                        label: 'Person Detected',
                        data: filteredData.map(d => d.person ? 1 : 0),
                        type: 'bar',
                        backgroundColor: 'rgba(76, 175, 80, 0.3)',
                        borderColor: '#4CAF50',
                        borderWidth: 1
                    }}]
                }},
                options: commonOptions
            }});

            // Color Chart
            const colCtx = document.getElementById('colorChart').getContext('2d');
            colorChart = new Chart(colCtx, {{
                type: 'line',
                data: {{
                    labels: labels,
                    datasets: [{{
                        label: 'Color Score',
                        data: filteredData.map(d => d.color_score),
                        borderColor: '#9C27B0',
                        backgroundColor: 'rgba(156, 39, 176, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4
                    }}]
                }},
                options: commonOptions
            }});

            // Confidence Chart
            const confCtx = document.getElementById('confidenceChart').getContext('2d');
            confidenceChart = new Chart(confCtx, {{
                type: 'line',
                data: {{
                    labels: labels,
                    datasets: [{{
                        label: 'Person Confidence',
                        data: filteredData.map(d => d.person_confidence),
                        borderColor: '#4CAF50',
                        backgroundColor: 'rgba(76, 175, 80, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4
                    }}, {{
                        label: 'Light Confidence',
                        data: filteredData.map(d => d.light_confidence),
                        borderColor: '#FFC107',
                        backgroundColor: 'rgba(255, 193, 7, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4
                    }}]
                }},
                options: commonOptions
            }});
        }}

        function updateTimeline() {{
            const timeline = document.getElementById('timeline');
            timeline.innerHTML = '';

            filteredData.forEach(d => {{
                const frame = document.createElement('div');
                frame.className = 'timeline-frame ' + (d.person ? 'person' : 'no-person');
                frame.title = `Frame ${{d.frame}}\nPerson: ${{d.person ? 'Yes' : 'No'}}\nLight: ${{d.light ? 'ON' : 'OFF'}}`;

                frame.addEventListener('mouseenter', (e) => showTooltip(e, d));
                frame.addEventListener('mouseleave', hideTooltip);

                timeline.appendChild(frame);
            }});
        }}

        function showTooltip(e, data) {{
            const tooltip = document.getElementById('tooltip');
            tooltip.innerHTML = `
                <strong>Frame ${{data.frame}}</strong><br>
                Time: ${{data.time_sec.toFixed(2)}}s<br>
                Person: ${{data.person ? 'YES' : 'NO'}} (Conf: ${{(data.person_confidence * 100).toFixed(1)}}%)<br>
                Light: ${{data.light ? 'ON' : 'OFF'}} (Conf: ${{(data.light_confidence * 100).toFixed(1)}}%)<br>
                Brightness: ${{data.light_brightness.toFixed(1)}}<br>
                Motion: ${{data.motion_score.toFixed(4)}}<br>
                Color: ${{data.color_score.toFixed(4)}}
            `;
            tooltip.style.display = 'block';
            tooltip.style.left = e.clientX + 10 + 'px';
            tooltip.style.top = e.clientY + 10 + 'px';
        }}

        function hideTooltip() {{
            document.getElementById('tooltip').style.display = 'none';
        }}

        function updateCharts() {{
            const labels = filteredData.map(d => d.frame);

            brightnessChart.data.labels = labels;
            brightnessChart.data.datasets[0].data = filteredData.map(d => d.light_brightness);
            brightnessChart.update();

            motionChart.data.labels = labels;
            motionChart.data.datasets[0].data = filteredData.map(d => d.motion_score);
            motionChart.data.datasets[1].data = filteredData.map(d => d.person ? 1 : 0);
            motionChart.update();

            colorChart.data.labels = labels;
            colorChart.data.datasets[0].data = filteredData.map(d => d.color_score);
            colorChart.update();

            confidenceChart.data.labels = labels;
            confidenceChart.data.datasets[0].data = filteredData.map(d => d.person_confidence);
            confidenceChart.data.datasets[1].data = filteredData.map(d => d.light_confidence);
            confidenceChart.update();

            updateTimeline();
        }}

        function applyFilters() {{
            const step = parseInt(document.getElementById('frameStep').value);
            const personOnly = document.getElementById('personOnly').checked;

            document.getElementById('stepValue').textContent = step;

            filteredData = allData.filter((d, index) => {{
                if (personOnly && !d.person) return false;
                return index % step === 0;
            }});

            updateCharts();
        }}

        function resetFilters() {{
            document.getElementById('frameStep').value = 1;
            document.getElementById('personOnly').checked = false;
            document.getElementById('stepValue').textContent = '1';
            filteredData = [...allData];
            updateCharts();
        }}

        // Event listeners
        document.getElementById('frameStep').addEventListener('input', applyFilters);
        document.getElementById('personOnly').addEventListener('change', applyFilters);

        // Initialize
        initCharts();
        updateTimeline();
    </script>
</body>
</html>
"""

    with open(output_path, 'w', encoding='utf-8') as f:
        f.write(html_content)

    return True


def main():
    import argparse

    parser = argparse.ArgumentParser(description='EcoAlert MATLAB Algorithm Visualizer')
    parser.add_argument('--video', '-v', required=True, help='Path to video file')
    parser.add_argument('--person-threshold', type=float, default=0.65,
                       help='Person detection threshold (default: 0.65)')
    parser.add_argument('--light-threshold', type=float, default=0.70,
                       help='Light detection threshold (default: 0.70)')
    parser.add_argument('--frame-step', type=int, default=10,
                       help='Process every Nth frame (default: 10)')
    parser.add_argument('--output', '-o', default='dashboard.html',
                       help='Output HTML file path (default: dashboard.html)')

    args = parser.parse_args()

    script_dir = Path(__file__).parent
    csv_path = script_dir / 'analysis_results.csv'

    print("=" * 60)
    print("EcoAlert MATLAB Algorithm Visualizer")
    print("=" * 60)

    # Step 1: Run MATLAB analysis
    success = run_matlab_analysis(
        args.video,
        args.person_threshold,
        args.light_threshold,
        args.frame_step
    )

    if not success:
        print("\nFailed to run MATLAB analysis")
        sys.exit(1)

    # Step 2: Generate HTML dashboard
    if not csv_path.exists():
        print(f"\nError: Results file not found at {csv_path}")
        sys.exit(1)

    print("\nGenerating HTML dashboard...")
    output_path = Path(args.output)
    if not output_path.is_absolute():
        output_path = script_dir / output_path

    success = generate_html_dashboard(str(csv_path), str(output_path))

    if success:
        print(f"\nDashboard generated successfully!")
        print(f"Open this file in your browser: {output_path}")

        # Try to open in browser automatically
        try:
            import webbrowser
            webbrowser.open(str(output_path))
            print("Opening in browser...")
        except:
            pass
    else:
        print("\nFailed to generate dashboard")
        sys.exit(1)


if __name__ == '__main__':
    main()
