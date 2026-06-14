% EcoAlertDetector Visualization Script
% Provides real-time visualization of light and person detection results
% with plots for key metrics and ROI overlay

clear; clc; close all;

%% Configuration
videoPath = "G:\project\EcoAlert\Video\5·28域控.mp4";
frameStep = 10;           % Process every Nth frame for performance
maxFramesToAnalyze = inf; % Set finite number for testing

% Detection thresholds
personThreshold = 0.65;
lightThreshold = 0.70;

% Initialize detector
detector = EcoAlertDetector(personThreshold, lightThreshold);

% ROI configuration (optional - set to default or customize)
roiConfig = EcoAlertDetector.defaultRoiConfig();
% Example custom ROI:
% roiConfig.light_rois = EcoAlertDetector.roiRect(0.20, 0.00, 0.60, 0.40, "lamp");
% roiConfig.light_on_threshold = 0.055;
% roiConfig.light_off_threshold = 0.025;

%% Validate video path
if strlength(videoPath) == 0 || ~exist(videoPath, 'file')
    error("Video file not found: %s", videoPath);
end

%% Initialize video reader
reader = VideoReader(videoPath);
totalFrames = reader.NumFrames;
fps = reader.FrameRate;
duration = reader.Duration;

fprintf('Video Info:\n');
fprintf('  File: %s\n', videoPath);
fprintf('  Resolution: %dx%d\n', reader.Width, reader.Height);
fprintf('  FPS: %.2f\n', fps);
fprintf('  Duration: %.2f seconds\n', duration);
fprintf('  Total frames: %d\n\n', totalFrames);

%% Setup visualization figure
fig = figure('Name', 'EcoAlert Real-time Analysis', ...
    'Position', [100, 100, 1400, 900], ...
    'Color', 'w', ...
    'NumberTitle', 'off');

% Top-left: Video frame with ROI overlay
axVideo = subplot(2, 3, [1, 2]);
axis(axVideo, 'equal');
axis(axVideo, 'off');
title(axVideo, 'Video Frame with Detection Overlay', 'FontSize', 12, 'FontWeight', 'bold');
imgHandle = imshow(uint8(zeros(reader.Height, reader.Width, 3, 'uint8')), 'Parent', axVideo);

% Text overlay handles on video axes
txtPerson = text(10, 30, '', 'Parent', axVideo, 'Color', 'w', ...
    'FontSize', 14, 'FontWeight', 'bold', 'BackgroundColor', 'k', ...
    'EdgeColor', 'none', 'Margin', 3);
txtLight = text(10, 55, '', 'Parent', axVideo, 'Color', 'w', ...
    'FontSize', 14, 'FontWeight', 'bold', 'BackgroundColor', 'k', ...
    'EdgeColor', 'none', 'Margin', 3);
txtMetrics = text(10, 80, '', 'Parent', axVideo, 'Color', 'w', ...
    'FontSize', 10, 'BackgroundColor', 'k', 'EdgeColor', 'none', 'Margin', 3);

% ROI rectangle handle (created on first frame with ROIs)
roiRectHandle = [];

% Top-right: Light status indicator
axLightStatus = subplot(2, 3, 3);
axis(axLightStatus, 'off');
title(axLightStatus, 'Light Status', 'FontSize', 12, 'FontWeight', 'bold');
lightTextHandle = text(0.5, 0.7, 'OFF', ...
    'Parent', axLightStatus, ...
    'HorizontalAlignment', 'center', ...
    'VerticalAlignment', 'middle', ...
    'FontSize', 36, ...
    'FontWeight', 'bold', ...
    'Color', 'r');
lightConfText = text(0.5, 0.3, 'Confidence: 0.00', ...
    'Parent', axLightStatus, ...
    'HorizontalAlignment', 'center', ...
    'FontSize', 10, ...
    'Color', 'k');

% Bottom row: Metric plots
axBrightness = subplot(2, 3, 4);
title(axBrightness, 'Brightness Over Time', 'FontSize', 10);
xlabel(axBrightness, 'Frame');
ylabel(axBrightness, 'Brightness');
grid(axBrightness, 'on');
hold(axBrightness, 'on');
brightnessPlot = plot(axBrightness, NaN, 'b-', 'LineWidth', 1.5);
ylim(axBrightness, [0, 255]);
yline(axBrightness, lightThreshold * 255, 'r--', 'DisplayName', 'Threshold');

axColor = subplot(2, 3, 5);
title(axColor, 'Color Score Over Time', 'FontSize', 10);
xlabel(axColor, 'Frame');
ylabel(axColor, 'Color Score');
grid(axColor, 'on');
hold(axColor, 'on');
colorPlot = plot(axColor, NaN, 'm-', 'LineWidth', 1.5);
ylim(axColor, [0, 0.2]);

axMotion = subplot(2, 3, 6);
title(axMotion, 'Motion Score Over Time', 'FontSize', 10);
xlabel(axMotion, 'Frame');
ylabel(axMotion, 'Motion Score');
grid(axMotion, 'on');
hold(axMotion, 'on');
motionPlot = plot(axMotion, NaN, 'g-', 'LineWidth', 1.5);
ylim(axMotion, [0, 1]);

% Person detection scatter handle (hidden initially)
personScatter = scatter(axMotion, NaN, NaN, 50, 'r', 'filled', ...
    'MarkerFaceAlpha', 0.6, 'DisplayName', 'Person Detected', 'Visible', 'off');

%% Data storage for plots
frames = [];
brightnessValues = [];
colorScores = [];
motionScores = [];
personDetections = [];
lightStates = [];

%% Processing loop
frameIndex = 0;
analyzed = 0;
startTime = tic;

fprintf('Starting analysis...\nPress Ctrl+C to stop.\n\n');

try
    while hasFrame(reader) && analyzed < maxFramesToAnalyze
        frame = readFrame(reader);
        frameIndex = frameIndex + 1;

        % Skip frames based on frameStep
        if mod(frameIndex - 1, frameStep) ~= 0
            continue;
        end

        % Run detection
        result = detector.analyzeScene(frame, roiConfig, "ColorOrder", "rgb");
        scene = result.scene;
        analyzed = analyzed + 1;

        % Store data
        frames(end+1) = frameIndex;
        brightnessValues(end+1) = scene.light_brightness;
        colorScores(end+1) = scene.color_score;
        motionScores(end+1) = scene.motion_score;
        personDetections(end+1) = double(scene.person);
        lightStates(end+1) = double(scene.light);

        % --- Update video display ---
        displayFrame = frame;

        % Draw ROI rectangles if configured
        rois = EcoAlertDetector.getLightRois(roiConfig);
        if ~isempty(rois)
            % Remove old ROI rectangle
            if ~isempty(roiRectHandle) && ishandle(roiRectHandle)
                delete(roiRectHandle);
            end
            roiRectHandle = [];
            for i = 1:numel(rois)
                bounds = EcoAlertDetector.roiBounds(size(frame, 2), size(frame, 1), rois(i));
                if ~isempty(bounds)
                    rx = bounds(1);
                    ry = bounds(2);
                    rw = bounds(3) - bounds(1) + 1;
                    rh = bounds(4) - bounds(2) + 1;
                    roiRectHandle = rectangle('Position', [rx, ry, rw, rh], ...
                        'EdgeColor', 'g', 'LineWidth', 2, 'Parent', axVideo);
                end
            end
        end

        % Update text overlays on video
        if scene.person
            set(txtPerson, 'String', 'PERSON DETECTED', 'Color', [0 1 0]);
        else
            set(txtPerson, 'String', 'No Person', 'Color', [1 1 1]);
        end

        if scene.light
            set(txtLight, 'String', 'LIGHT ON', 'Color', [1 1 0]);
        else
            set(txtLight, 'String', 'LIGHT OFF', 'Color', [1 0 0]);
        end

        set(txtMetrics, 'String', ...
            sprintf('Bri: %.1f | Color: %.4f | Motion: %.4f', ...
            scene.light_brightness, scene.color_score, scene.motion_score));

        % Update image
        set(imgHandle, 'CData', displayFrame);

        % Update light status panel
        if scene.light
            set(lightTextHandle, 'String', 'ON', 'Color', [0, 0.8, 0]);
        else
            set(lightTextHandle, 'String', 'OFF', 'Color', [0.8, 0, 0]);
        end
        set(lightConfText, 'String', sprintf('Confidence: %.2f', scene.light_confidence));

        % --- Update plots ---
        set(brightnessPlot, 'XData', frames, 'YData', brightnessValues);
        set(colorPlot, 'XData', frames, 'YData', colorScores);
        set(motionPlot, 'XData', frames, 'YData', motionScores);

        % Adjust plot x-limits dynamically
        if ~isempty(frames)
            xlimRange = [min(frames), max(frames)];
            xlim(axBrightness, xlimRange);
            xlim(axColor, xlimRange);
            xlim(axMotion, xlimRange);
        end

        % Update person detection markers on motion plot
        personIdx = find(personDetections == 1);
        if ~isempty(personIdx)
            set(personScatter, 'XData', frames(personIdx), ...
                'YData', motionScores(personIdx), 'Visible', 'on');
        else
            set(personScatter, 'Visible', 'off');
        end

        % Force MATLAB to update the display
        drawnow limitrate;

        % Print progress
        if mod(analyzed, 10) == 0
            elapsed = toc(startTime);
            progress = min(analyzed / (totalFrames / frameStep) * 100, 100);
            fprintf('[%.1f%%] Frame %d/%d | Time: %.1fs | Person: %d | Light: %d | Bri: %.1f | Motion: %.4f\n', ...
                progress, frameIndex, totalFrames, elapsed, ...
                scene.person, scene.light, scene.light_brightness, scene.motion_score);
        end
    end

catch ME
    fprintf('\nAnalysis interrupted: %s\n', ME.message);
end

%% Final summary
elapsed = toc(startTime);
fprintf('\n\n===== Analysis Complete =====\n');
fprintf('Total frames analyzed: %d\n', analyzed);
fprintf('Processing time: %.2f seconds\n', elapsed);
if elapsed > 0
    fprintf('Average FPS: %.2f\n', analyzed / elapsed);
end
if ~isempty(personDetections)
    fprintf('Person detected in %d/%d frames (%.1f%%)\n', ...
        sum(personDetections), length(personDetections), ...
        sum(personDetections) / length(personDetections) * 100);
end
if ~isempty(lightStates)
    fprintf('Light ON in %d/%d frames (%.1f%%)\n', ...
        sum(lightStates), length(lightStates), ...
        sum(lightStates) / length(lightStates) * 100);
end

%% Export results to table
if ~isempty(frames)
    resultTable = table( ...
        frames', ...
        brightnessValues', ...
        colorScores', ...
        motionScores', ...
        personDetections', ...
        lightStates', ...
        'VariableNames', {'Frame', 'Brightness', 'ColorScore', 'MotionScore', 'PersonDetected', 'LightOn'});

    exportPath = fullfile(pwd, 'ecoalert_analysis_results.csv');
    writetable(resultTable, exportPath);
    fprintf('\nResults exported to: %s\n', exportPath);
end
