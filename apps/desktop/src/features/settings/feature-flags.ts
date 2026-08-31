const RICH_COMPARISON_PICKER_KEY = "trace.feature.rich-comparison-picker";
export const FEATURE_FLAGS_CHANGED_EVENT = "trace:feature-flags-changed";

export function richComparisonPickerEnabled() {
	return localStorage.getItem(RICH_COMPARISON_PICKER_KEY) !== "false";
}

export function setRichComparisonPickerEnabled(enabled: boolean) {
	localStorage.setItem(RICH_COMPARISON_PICKER_KEY, String(enabled));
	window.dispatchEvent(new Event(FEATURE_FLAGS_CHANGED_EVENT));
}
