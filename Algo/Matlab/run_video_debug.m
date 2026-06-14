% Debug the current EcoAlert app light/person proxy detector in MATLAB.
%
% Edit videoPath and roiConfig, then run this script. The output table uses
% the same core fields shown by the app real-time card.

clear; clc;

videoPath = "G:\project\EcoAlert\Video\5·28域控.mp4";           % Example: "G:\project\EcoAlert\Video\sample.mp4"
frameStep = 25;           % Analyze every Nth decoded frame.
maxFramesToAnalyze = inf; % Set a finite number for quick threshold tuning.

detector = EcoAlertDetector(0.65, 0.70);

roiConfig = EcoAlertDetector.defaultRoiConfig();
% Example ROI in normalized coordinates:
% roiConfig.light_rois = EcoAlertDetector.roiRect(0.20, 0.00, 0.60, 0.40, "lamp");
% roiConfig.light_on_threshold = 0.055;
% roiConfig.light_off_threshold = 0.025;

if strlength(videoPath) == 0
    error("Set videoPath before running run_video_debug.m");
end

reader = VideoReader(videoPath);
rows = {};
frameIndex = 0;
analyzed = 0;

while hasFrame(reader) && analyzed < maxFramesToAnalyze
    frame = readFrame(reader); % MATLAB returns RGB.
    frameIndex = frameIndex + 1;
    if mod(frameIndex - 1, frameStep) ~= 0
        continue;
    end

    result = detector.analyzeScene(frame, roiConfig, "ColorOrder", "rgb");
    scene = result.scene;
    analyzed = analyzed + 1;

    rows(end + 1, :) = { ...
        frameIndex, reader.CurrentTime, scene.person, scene.light, ...
        scene.person_confidence, scene.light_confidence, ...
        scene.light_brightness, scene.color_score, scene.motion_score, ...
        scene.reason}; %#ok<SAGROW>

    fprintf("frame=%d t=%.2fs person=%d light=%d color=%.4f motion=%.4f brightness=%.1f reason=%s\n", ...
        frameIndex, reader.CurrentTime, scene.person, scene.light, ...
        scene.color_score, scene.motion_score, scene.light_brightness, scene.reason);
end

resultTable = cell2table(rows, "VariableNames", { ...
    "frame", "time_sec", "person", "light", "person_confidence", ...
    "light_confidence", "light_brightness", "color_score", ...
    "motion_score", "reason"});

disp(resultTable);

% Optional export:
% writetable(resultTable, "ecoalert_matlab_debug.csv");
