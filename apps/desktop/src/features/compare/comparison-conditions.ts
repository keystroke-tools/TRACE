import type { RecordedSessionSummary } from "../../data-source";

export type ComparisonConditionWarning = {
	title: string;
	details: string[];
};

const WET_WEATHER = /\b(rain|wet|storm|drizzle|shower|monsoon)\b/i;
const DRY_WEATHER = /\b(clear|dry|fair|sun|sunny)\b/i;

export function comparisonConditionWarning(
	reference?: RecordedSessionSummary | null,
	analysed?: RecordedSessionSummary | null,
): ComparisonConditionWarning | null {
	if (!reference || !analysed) return null;
	const details: string[] = [];
	const referenceWeather = normalizedWeather(reference.weatherName);
	const analysedWeather = normalizedWeather(analysed.weatherName);
	const wetDryMismatch =
		referenceWeather != null &&
		analysedWeather != null &&
		((WET_WEATHER.test(referenceWeather) && DRY_WEATHER.test(analysedWeather)) ||
			(DRY_WEATHER.test(referenceWeather) && WET_WEATHER.test(analysedWeather)));
	if (referenceWeather && analysedWeather && referenceWeather !== analysedWeather) {
		details.push(`Weather ${friendlyWeather(referenceWeather)} → ${friendlyWeather(analysedWeather)}`);
	}
	addNumericDifference(details, "Track", reference.roadTemperatureC, analysed.roadTemperatureC, 5, "°C");
	addNumericDifference(details, "Air", reference.ambientTemperatureC, analysed.ambientTemperatureC, 3, "°C");
	addNumericDifference(details, "Grip", reference.trackGripPercent, analysed.trackGripPercent, 5, "%");
	if (details.length === 0) return null;
	return {
		title: wetDryMismatch ? "Wet and dry laps compared" : "Session conditions differ",
		details,
	};
}

function addNumericDifference(
	details: string[],
	label: string,
	reference: string | number | null | undefined,
	analysed: string | number | null | undefined,
	threshold: number,
	unit: string,
) {
	const referenceValue = numericValue(reference);
	const analysedValue = numericValue(analysed);
	if (referenceValue == null || analysedValue == null || Math.abs(referenceValue - analysedValue) < threshold) return;
	details.push(`${label} ${formatNumber(referenceValue)}${unit} → ${formatNumber(analysedValue)}${unit}`);
}

function numericValue(value: string | number | null | undefined) {
	if (typeof value === "number") return Number.isFinite(value) ? value : null;
	if (typeof value !== "string") return null;
	const parsed = Number.parseFloat(value);
	return Number.isFinite(parsed) ? parsed : null;
}

function normalizedWeather(value?: string | null) {
	const normalized = value?.trim().toLowerCase().replace(/[_-]+/g, " ").replace(/\s+/g, " ");
	return normalized || null;
}

function friendlyWeather(value: string) {
	return value.replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function formatNumber(value: number) {
	return Number.isInteger(value) ? String(value) : value.toFixed(1);
}
