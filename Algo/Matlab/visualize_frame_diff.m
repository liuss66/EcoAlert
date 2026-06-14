% EcoAlert Frame Difference Visualization
% 可视化帧差法的详细过程:帧差图像、运动热力图、分数曲线

clear; clc; close all;

%% Configuration
videoPath = "G:\project\EcoAlert\Video\5·15底盘.mp4";
frameStep = 5;           % 每N帧分析一次
maxFrames = inf;       % 分析全部帧

% 算法参数(与EcoAlertDetector一致)
MOTION_WIDTH = 160;
MOTION_HEIGHT = 120;
MOTION_PIXEL_THRESHOLD = 20;
MOTION_AREA_THRESHOLD = 0.03;
EMA_ALPHA = 0.3;
personThreshold = 0.65;
effThreshold = personThreshold * MOTION_AREA_THRESHOLD;

%% Initialize
reader = VideoReader(videoPath);
fprintf('Video: %s\n', videoPath);
fprintf('Resolution: %dx%d, FPS: %.2f\n\n', reader.Width, reader.Height, reader.FrameRate);

%% Setup figure
fig = figure('Name', 'Frame Difference Visualization', ...
    'Position', [50, 50, 1600, 900], 'Color', 'w', 'NumberTitle', 'off');

% 布局: 2行3列
% 第1行: 当前帧 | 上一帧 | 帧差热力图
% 第2行: 二值化掩码 | 运动分数曲线 | 统计信息

% Row 1
axCurrent = subplot(2, 3, 1);
title(axCurrent, 'Current Frame', 'FontSize', 11);
axis(axCurrent, 'off');
imgCurrent = imshow(uint8(zeros(reader.Height, reader.Width)), 'Parent', axCurrent);

axPrev = subplot(2, 3, 2);
title(axPrev, 'Previous Frame', 'FontSize', 11);
axis(axPrev, 'off');
imgPrev = imshow(uint8(zeros(reader.Height, reader.Width)), 'Parent', axPrev);

axDiff = subplot(2, 3, 3);
title(axDiff, 'Frame Difference (Original Resolution)', 'FontSize', 11);
axis(axDiff, 'off');
imgDiff = imshow(uint8(zeros(reader.Height, reader.Width)), 'Parent', axDiff);

% Row 2
axMask = subplot(2, 3, 4);
title(axMask, 'Motion Mask (threshold > 20)', 'FontSize', 11);
axis(axMask, 'off');
imgMask = imshow(zeros(MOTION_HEIGHT, MOTION_WIDTH), 'Parent', axMask);

axScore = subplot(2, 3, [5, 6]);
title(axScore, 'Motion Score Over Time', 'FontSize', 12, 'FontWeight', 'bold');
xlabel(axScore, 'Frame', 'FontSize', 10);
ylabel(axScore, 'Motion Score', 'FontSize', 10);
grid(axScore, 'on');
hold(axScore, 'on');

% 初始化曲线
hRawScore = plot(axScore, NaN, NaN, 'b-', 'LineWidth', 1.5, 'DisplayName', 'Raw Score');
hEmaScore = plot(axScore, NaN, NaN, 'r-', 'LineWidth', 2, 'DisplayName', 'EMA Score');
hThreshLine = yline(axScore, effThreshold, 'g--', 'LineWidth', 2, 'DisplayName', sprintf('Threshold=%.4f', effThreshold));
hPersonMarker = scatter(axScore, NaN, NaN, 80, 'r', 'filled', 'MarkerFaceAlpha', 0.6, ...
    'DisplayName', 'Person Detected', 'Visible', 'off');
legend(axScore, 'Location', 'northeast');
ylim(axScore, [0, 1]);

% 统计文本(用axes内text,确保getframe能捕获)
axStats = axes('Position', [0.01, 0.02, 0.28, 0.12], 'Parent', fig, ...
    'XLim', [0 1], 'YLim', [0 1], 'XTick', [], 'YTick', [], ...
    'Box', 'on', 'Color', [1 1 0.9]);
txtStats = text(0.05, 0.5, '', 'Parent', axStats, ...
    'FontSize', 10, 'VerticalAlignment', 'middle', 'FontName', 'FixedWidth');

%% Video output
outputVideoPath = fullfile(pwd, 'frame_diff_output.mp4');
vWriter = VideoWriter(outputVideoPath, 'MPEG-4');
vWriter.FrameRate = 10;  % 输出视频帧率
vWriter.Quality = 95;
open(vWriter);
fprintf('Output video: %s\n', outputVideoPath);

%% Data storage
frameNums = [];
rawScores = [];
emaScores = [];
personFlags = [];

%% Processing loop
prevLowRes = [];
prevGray = [];
motionEma = 0;
frameIndex = 0;
analyzed = 0;

fprintf('Starting frame difference analysis...\n');
fprintf('Press Ctrl+C to stop.\n\n');

while hasFrame(reader) && analyzed < maxFrames
    frame = readFrame(reader);
    frameIndex = frameIndex + 1;
    
    if mod(frameIndex - 1, frameStep) ~= 0
        continue;
    end
    
    % 转灰度
    if ndims(frame) == 3
        gray = uint8(0.299 * double(frame(:,:,1)) + 0.587 * double(frame(:,:,2)) + 0.114 * double(frame(:,:,3)));
    else
        gray = frame;
    end
    
    % 降采样
    [h, w] = size(gray);
    xs = floor((0:MOTION_WIDTH-1) * w / MOTION_WIDTH) + 1;
    ys = floor((0:MOTION_HEIGHT-1) * h / MOTION_HEIGHT) + 1;
    currentLowRes = gray(ys, xs);
    
    analyzed = analyzed + 1;
    
    % 计算原始分辨率帧差(用于显示)
    if isempty(prevGray)
        diffImageOrig = zeros(size(gray), 'uint8');
    else
        diffImageOrig = uint8(abs(double(gray) - double(prevGray)));
    end
    
    % 计算低分辨率帧差(用于运动检测)
    if isempty(prevLowRes)
        diffImage = zeros(MOTION_HEIGHT, MOTION_WIDTH);
        motionMask = zeros(MOTION_HEIGHT, MOTION_WIDTH);
        rawScore = 0;
    else
        diffImage = abs(double(currentLowRes) - double(prevLowRes));
        motionMask = double(diffImage > MOTION_PIXEL_THRESHOLD);
        rawScore = sum(motionMask(:)) / numel(motionMask);
    end
    
    % EMA平滑
    motionEma = motionEma * (1 - EMA_ALPHA) + rawScore * EMA_ALPHA;
    
    % 人员判定
    personDetected = motionEma >= effThreshold;
    
    % 存储数据
    frameNums(end+1) = frameIndex;
    rawScores(end+1) = rawScore;
    emaScores(end+1) = motionEma;
    personFlags(end+1) = double(personDetected);
    
    % 更新显示 (使用原始分辨率)
    set(imgCurrent, 'CData', gray);
    if ~isempty(prevGray)
        set(imgPrev, 'CData', prevGray);
    end
    
    % 帧差热力图 (原始分辨率,不压缩)
    set(imgDiff, 'CData', diffImageOrig);
    
    % 运动掩码
    set(imgMask, 'CData', motionMask);
    
    % 更新曲线
    set(hRawScore, 'XData', frameNums, 'YData', rawScores);
    set(hEmaScore, 'XData', frameNums, 'YData', emaScores);
    if length(frameNums) > 1
        xlim(axScore, [min(frameNums), max(frameNums)]);
    end
    
    % 人员检测标记
    personIdx = find(personFlags == 1);
    if ~isempty(personIdx)
        set(hPersonMarker, 'XData', frameNums(personIdx), ...
            'YData', emaScores(personIdx), 'Visible', 'on');
    end
    
    % 更新统计文本
    personStr = 'NO';
    if personDetected
        personStr = 'YES !!!';
    end
    set(txtStats, 'String', sprintf(...
        'Frame: %d / %d\nRaw: %.4f  EMA: %.4f\nThresh: %.4f\nPerson: %s', ...
        frameIndex, analyzed, rawScore, motionEma, effThreshold, personStr));
    
    % 控制台输出
    if mod(analyzed, 10) == 0
        fprintf('[Frame %d] Raw=%.4f EMA=%.4f Person=%d\n', ...
            frameIndex, rawScore, motionEma, personDetected);
    end
    
    % 保存上一帧
    prevLowRes = currentLowRes;
    prevGray = gray;
    
    % 写入视频帧
    writeVideo(vWriter, getframe(fig));
    
    drawnow limitrate;
end

%% Final summary
fprintf('\n===== Analysis Complete =====\n');
fprintf('Total frames: %d, Analyzed: %d\n', frameIndex, analyzed);
fprintf('Person detected: %d/%d frames (%.1f%%)\n', ...
    sum(personFlags), length(personFlags), sum(personFlags)/length(personFlags)*100);
fprintf('Mean raw score: %.4f, Mean EMA: %.4f\n', mean(rawScores), mean(emaScores));
fprintf('Max raw score: %.4f, Max EMA: %.4f\n', max(rawScores), max(emaScores));

%% 导出结果
resultTable = table(frameNums', rawScores', emaScores', personFlags', ...
    'VariableNames', {'Frame', 'RawScore', 'EmaScore', 'PersonDetected'});
writetable(resultTable, 'frame_diff_analysis.csv');
fprintf('\nResults saved to: frame_diff_analysis.csv\n');

%% 关闭视频
close(vWriter);
fprintf('Video saved to: %s\n', outputVideoPath);
fprintf('Duration: %.1f seconds (%d frames @ %d fps)\n', ...
    length(frameNums) / vWriter.FrameRate, length(frameNums), vWriter.FrameRate);
