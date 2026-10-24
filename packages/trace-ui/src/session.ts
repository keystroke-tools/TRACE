export interface SessionIdentity {
	title?: string | null;
	driver?: string | null;
	track?: string | null;
	sessionType?: string | null;
	startedAt?: string | null;
}

export function formatSessionType(value?: string | null) {
	const type = value?.trim().toLocaleLowerCase() ?? "";
	if (!type) return "Unknown";
	if (type === "session") return "Drive";
	if (type === "qualify") return "Qualifying";
	if (type.includes("replay")) return "Replay";
	return type
		.split(/[\s_-]+/)
		.filter(Boolean)
		.map((part) => `${part[0]?.toLocaleUpperCase() ?? ""}${part.slice(1).toLocaleLowerCase()}`)
		.join(" ");
}

export function formatSessionDate(value?: string | null, locale?: string) {
	if (!value) return "Unknown date";
	const date = new Date(value);
	if (Number.isNaN(date.valueOf())) return value;
	return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export function sessionDisplayName(session: SessionIdentity) {
	return session.title?.trim() || session.driver?.trim() || session.track?.trim() || "Recorded session";
}

export function sessionUrlSlug(session: SessionIdentity) {
	const identity = session.driver?.trim() || session.title?.trim() || "session";
	const slug = `${identity} ${session.track ?? ""}`
		.normalize("NFKD")
		.toLocaleLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "")
		.slice(0, 80)
		.replace(/-$/, "");
	return slug || "session";
}
