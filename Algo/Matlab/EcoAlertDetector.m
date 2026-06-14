classdef EcoAlertDetector < handle
    %EcoAlertDetector MATLAB mirror of the current EcoAlert app detector.
    %
    % The app currently uses:
    % - RGB chroma score for light on/off when RGB frames are available.
    % - Brightness threshold fallback for grayscale-only frames.
    % - Frame-difference motion score as the temporary person proxy.

    properties
        personThreshold double = 0.65
        lightThreshold double = 0.70
    end

    properties (Access = private)
        prevLowRes uint8 = uint8([])
        lightState logical = false
        brightnessEma double = NaN
        colorEma double = NaN
        motionEma double = 0
        frameCounter double = 0
    end

    properties (Constant, Access = private)
        MOTION_WIDTH double = 160
        MOTION_HEIGHT double = 120
        MOTION_PIXEL_THRESHOLD double = 20
        MOTION_AREA_THRESHOLD double = 0.03
        EMA_ALPHA double = 0.3
        COLOR_ON_THRESHOLD double = 0.055
        COLOR_OFF_THRESHOLD double = 0.025
        COLOR_DARK_LUMA_CUTOFF double = 24.0
        COLOR_WEIGHT_LUMA_FLOOR double = 40.0
    end

    methods
        function obj = EcoAlertDetector(personThreshold, lightThreshold)
            if nargin >= 1 && ~isempty(personThreshold)
                obj.personThreshold = personThreshold;
            end
            if nargin >= 2 && ~isempty(lightThreshold)
                obj.lightThreshold = lightThreshold;
            end
        end

        function reset(obj)
            obj.prevLowRes = uint8([]);
            obj.lightState = false;
            obj.brightnessEma = NaN;
            obj.colorEma = NaN;
            obj.motionEma = 0;
            obj.frameCounter = 0;
        end

        function setThresholds(obj, personThreshold, lightThreshold)
            obj.personThreshold = personThreshold;
            obj.lightThreshold = lightThreshold;
        end

        function threshold = effectivePersonThreshold(obj)
            threshold = max(EcoAlertDetector.clamp(obj.personThreshold, 0.05, 1.0) ...
                * obj.MOTION_AREA_THRESHOLD, 0.001);
        end

        function result = analyzeScene(obj, frame, roiConfig, varargin)
            % analyzeScene(frame, roiConfig, "ColorOrder", "rgb"|"bgr")
            %
            % frame:
            %   HxW uint8/double grayscale, or HxWx3 RGB image by default.
            % roiConfig:
            %   struct with light_rois/lightRois and thresholds, or [].
            started = tic;
            if nargin < 3
                roiConfig = [];
            end

            p = inputParser;
            addParameter(p, "ColorOrder", "rgb");
            parse(p, varargin{:});
            colorOrder = lower(string(p.Results.ColorOrder));

            obj.frameCounter = obj.frameCounter + 1;
            [gray, rgb, hasColorSignal] = EcoAlertDetector.normalizeFrame(frame, colorOrder);

            brightness = EcoAlertDetector.averageLightBrightness(gray, roiConfig);
            smoothedBrightness = EcoAlertDetector.ema(obj.brightnessEma, brightness);
            obj.brightnessEma = smoothedBrightness;

            colorScore = EcoAlertDetector.averageColorScore(rgb, gray, roiConfig);
            smoothedColor = EcoAlertDetector.ema(obj.colorEma, colorScore);
            obj.colorEma = smoothedColor;

            [colorOnThreshold, colorOffThreshold] = EcoAlertDetector.colorThresholds(roiConfig);
            brightnessOnThreshold = obj.lightThreshold;
            brightnessOffThreshold = obj.lightThreshold * 0.65;
            brightnessNorm = EcoAlertDetector.clamp(smoothedBrightness / 255.0, 0.0, 1.0);

            if hasColorSignal
                if smoothedColor >= colorOnThreshold
                    obj.lightState = true;
                elseif smoothedColor <= colorOffThreshold
                    obj.lightState = false;
                end
                lightConfidence = EcoAlertDetector.colorLightConfidence( ...
                    smoothedColor, obj.lightState, colorOnThreshold, colorOffThreshold);
                lightBy = "color";
            else
                if brightnessNorm >= brightnessOnThreshold
                    obj.lightState = true;
                elseif brightnessNorm <= brightnessOffThreshold
                    obj.lightState = false;
                end
                lightConfidence = EcoAlertDetector.lightConfidence( ...
                    brightnessNorm, obj.lightState, brightnessOnThreshold, brightnessOffThreshold);
                lightBy = "brightness";
            end

            motionRaw = obj.motionScore(gray);
            obj.motionEma = obj.motionEma * (1.0 - obj.EMA_ALPHA) + motionRaw * obj.EMA_ALPHA;

            effThreshold = obj.effectivePersonThreshold();
            person = effThreshold > 0.0 && obj.motionEma >= effThreshold;
            if effThreshold > 0.0
                if person
                    personConfidence = 0.5 + 0.5 * min((obj.motionEma - effThreshold) / effThreshold, 1.0);
                else
                    personConfidence = 0.5 * (obj.motionEma / effThreshold);
                end
            else
                personConfidence = 0.0;
            end

            processMs = toc(started) * 1000.0;
            if person
                reasonPrefix = "simple_motion_proxy";
            else
                reasonPrefix = "simple_no_motion";
            end

            scene = struct( ...
                "person", logical(person), ...
                "light", logical(obj.lightState), ...
                "frame_seq", obj.frameCounter, ...
                "confidence", lightConfidence, ...
                "source", "simple", ...
                "person_confidence", EcoAlertDetector.clamp(personConfidence, 0.0, 1.0), ...
                "light_confidence", lightConfidence, ...
                "reason", reasonPrefix + ";light_by_" + lightBy, ...
                "model_latency_ms", floor(processMs), ...
                "light_brightness", smoothedBrightness, ...
                "color_score", smoothedColor, ...
                "motion_score", obj.motionEma, ...
                "process_ms", processMs);

            result = struct( ...
                "scene", scene, ...
                "light_brightness", smoothedBrightness, ...
                "motion_score", obj.motionEma, ...
                "process_ms", processMs);
        end
    end

    methods (Access = private)
        function score = motionScore(obj, gray)
            if isempty(gray)
                score = 0.0;
                return;
            end

            [h, w] = size(gray);
            outW = max(1, min(obj.MOTION_WIDTH, w));
            outH = max(1, min(obj.MOTION_HEIGHT, h));
            xs = floor((0:outW - 1) * w / outW) + 1;
            ys = floor((0:outH - 1) * h / outH) + 1;
            small = gray(ys, xs);

            if isempty(obj.prevLowRes) || ~isequal(size(obj.prevLowRes), size(small))
                obj.prevLowRes = small;
                score = 0.0;
                return;
            end

            diff = abs(double(small) - double(obj.prevLowRes));
            changed = nnz(diff > obj.MOTION_PIXEL_THRESHOLD);
            obj.prevLowRes = small;
            score = changed / numel(small);
        end
    end

    methods (Static)
        function roiConfig = defaultRoiConfig()
            roiConfig = struct( ...
                "light_rois", [], ...
                "light_on_threshold", EcoAlertDetector.COLOR_ON_THRESHOLD, ...
                "light_off_threshold", EcoAlertDetector.COLOR_OFF_THRESHOLD);
        end

        function roi = roiRect(x, y, w, h, label)
            if nargin < 5
                label = "";
            end
            roi = struct("x", x, "y", y, "w", w, "h", h, "label", string(label));
        end

        function [gray, rgb, hasColorSignal] = normalizeFrame(frame, colorOrder)
            if isempty(frame)
                gray = uint8([]);
                rgb = uint8([]);
                hasColorSignal = false;
                return;
            end

            if ~isa(frame, "uint8")
                if isfloat(frame) && max(frame(:)) <= 1.0
                    frame = uint8(round(EcoAlertDetector.clampArray(frame, 0.0, 1.0) * 255.0));
                else
                    frame = uint8(round(EcoAlertDetector.clampArray(double(frame), 0.0, 255.0)));
                end
            end

            if ndims(frame) == 2
                gray = frame;
                rgb = uint8([]);
                hasColorSignal = false;
                return;
            end

            rgb = frame(:, :, 1:3);
            if colorOrder == "bgr"
                rgb = rgb(:, :, [3 2 1]);
            end
            gray = EcoAlertDetector.rgbToGrayApp(rgb);
            hasColorSignal = true;
        end

        function gray = rgbToGrayApp(rgb)
            r = uint32(rgb(:, :, 1));
            g = uint32(rgb(:, :, 2));
            b = uint32(rgb(:, :, 3));
            gray = uint8(floor(double(77 * r + 150 * g + 29 * b) / 256.0));
        end

        function value = averageLightBrightness(gray, roiConfig)
            if isempty(gray)
                value = 0.0;
                return;
            end
            rois = EcoAlertDetector.getLightRois(roiConfig);
            values = [];
            for i = 1:numel(rois)
                bounds = EcoAlertDetector.roiBounds(size(gray, 2), size(gray, 1), rois(i));
                if isempty(bounds)
                    continue;
                end
                x1 = bounds(1); y1 = bounds(2); x2 = bounds(3); y2 = bounds(4);
                region = gray(y1:y2, x1:x2);
                values(end + 1) = mean(double(region(:))); %#ok<AGROW>
            end
            if ~isempty(values)
                value = mean(values);
            else
                value = mean(double(gray(:)));
            end
        end

        function value = averageColorScore(rgb, gray, roiConfig)
            if isempty(rgb)
                value = 0.0;
                return;
            end
            rois = EcoAlertDetector.getLightRois(roiConfig);
            scores = [];
            for i = 1:numel(rois)
                bounds = EcoAlertDetector.roiBounds(size(gray, 2), size(gray, 1), rois(i));
                if isempty(bounds)
                    continue;
                end
                x1 = bounds(1); y1 = bounds(2); x2 = bounds(3); y2 = bounds(4);
                score = EcoAlertDetector.colorScoreForRegion(rgb(y1:y2, x1:x2, :));
                if ~isnan(score)
                    scores(end + 1) = score; %#ok<AGROW>
                end
            end
            if ~isempty(scores)
                value = mean(scores);
            else
                value = EcoAlertDetector.colorScoreForRegion(rgb);
                if isnan(value)
                    value = 0.0;
                end
            end
        end

        function score = colorScoreForRegion(region)
            if isempty(region)
                score = NaN;
                return;
            end
            px = double(region);
            maxc = max(px, [], 3);
            minc = min(px, [], 3);
            luma = (77.0 * px(:, :, 1) + 150.0 * px(:, :, 2) + 29.0 * px(:, :, 3)) / 256.0;
            mask = luma >= EcoAlertDetector.COLOR_DARK_LUMA_CUTOFF;
            if ~any(mask(:))
                score = 0.0;
                return;
            end
            chroma = (maxc - minc) ./ max(maxc, 1.0);
            weight = EcoAlertDetector.clampArray( ...
                (luma - EcoAlertDetector.COLOR_WEIGHT_LUMA_FLOOR) ...
                / (255.0 - EcoAlertDetector.COLOR_WEIGHT_LUMA_FLOOR), 0.05, 1.0);
            score = sum(chroma(mask) .* weight(mask)) / sum(weight(mask));
        end

        function [onThreshold, offThreshold] = colorThresholds(roiConfig)
            if isempty(roiConfig)
                onThreshold = EcoAlertDetector.COLOR_ON_THRESHOLD;
                offThreshold = EcoAlertDetector.COLOR_OFF_THRESHOLD;
                return;
            end
            on = EcoAlertDetector.getField(roiConfig, ["light_on_threshold", "lightOnThreshold"], ...
                EcoAlertDetector.COLOR_ON_THRESHOLD);
            off = EcoAlertDetector.getField(roiConfig, ["light_off_threshold", "lightOffThreshold"], ...
                EcoAlertDetector.COLOR_OFF_THRESHOLD);
            if on > 0.2 || off > 0.2
                onThreshold = EcoAlertDetector.COLOR_ON_THRESHOLD;
                offThreshold = EcoAlertDetector.COLOR_OFF_THRESHOLD;
                return;
            end
            onThreshold = EcoAlertDetector.clamp(on, 0.0, 0.2);
            offThreshold = EcoAlertDetector.clamp(off, 0.0, onThreshold);
        end

        function rois = getLightRois(roiConfig)
            rois = [];
            if isempty(roiConfig)
                return;
            end
            if isfield(roiConfig, "light_rois")
                rois = roiConfig.light_rois;
            elseif isfield(roiConfig, "lightRois")
                rois = roiConfig.lightRois;
            end
        end

        function bounds = roiBounds(width, height, roi)
            if width <= 0 || height <= 0
                bounds = [];
                return;
            end
            x = EcoAlertDetector.getField(roi, "x", 0.0);
            y = EcoAlertDetector.getField(roi, "y", 0.0);
            w = EcoAlertDetector.getField(roi, "w", 0.0);
            h = EcoAlertDetector.getField(roi, "h", 0.0);
            x1z = floor(EcoAlertDetector.clamp(x, 0.0, 1.0) * width);
            y1z = floor(EcoAlertDetector.clamp(y, 0.0, 1.0) * height);
            x2z = ceil(EcoAlertDetector.clamp(x + w, 0.0, 1.0) * width);
            y2z = ceil(EcoAlertDetector.clamp(y + h, 0.0, 1.0) * height);
            if x2z <= x1z || y2z <= y1z
                bounds = [];
            else
                bounds = [x1z + 1, y1z + 1, x2z, y2z];
            end
        end

        function conf = lightConfidence(value, light, onThreshold, offThreshold)
            if light
                distance = max(value - offThreshold, 0.0);
            else
                distance = max(onThreshold - value, 0.0);
            end
            span = max(abs(onThreshold - offThreshold), 0.01);
            conf = EcoAlertDetector.clamp(distance / span, 0.0, 1.0);
        end

        function conf = colorLightConfidence(colorScore, light, onThreshold, offThreshold)
            if light
                distance = max(colorScore - offThreshold, 0.0);
            else
                distance = max(onThreshold - colorScore, 0.0);
            end
            span = max(onThreshold - offThreshold, 0.01);
            conf = EcoAlertDetector.clamp(distance / span, 0.0, 1.0);
        end

        function y = ema(prev, value)
            if isnan(prev)
                y = value;
            else
                y = prev * (1.0 - EcoAlertDetector.EMA_ALPHA) + value * EcoAlertDetector.EMA_ALPHA;
            end
        end

        function value = getField(s, names, fallback)
            value = fallback;
            if isempty(s)
                return;
            end
            names = string(names);
            for i = 1:numel(names)
                name = char(names(i));
                if isfield(s, name)
                    value = s.(name);
                    return;
                end
            end
        end

        function y = clamp(x, lo, hi)
            y = min(max(double(x), lo), hi);
        end

        function y = clampArray(x, lo, hi)
            y = min(max(x, lo), hi);
        end
    end
end
