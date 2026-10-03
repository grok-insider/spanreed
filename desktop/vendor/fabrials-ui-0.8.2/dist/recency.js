"use client";
const RECENCY_LABELS = {
  today: "Today",
  yesterday: "Yesterday",
  week: "Previous 7 days",
  month: "Previous 30 days",
  older: "Older"
};
const DAY = 24 * 60 * 60 * 1e3;
function calendarDay(date, timeZone) {
  if (!timeZone) return { year: date.getFullYear(), month: date.getMonth(), day: date.getDate() };
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone,
    year: "numeric",
    month: "numeric",
    day: "numeric"
  }).formatToParts(date);
  const read = (type) => Number(parts.find((part) => part.type === type)?.value);
  return { year: read("year"), month: read("month") - 1, day: read("day") };
}
function dayNumber({ year, month, day }) {
  return Math.round(Date.UTC(year, month, day) / DAY);
}
function toDate(value) {
  return value instanceof Date ? new Date(value.getTime()) : new Date(value);
}
function recencyGroupFor(value, now = /* @__PURE__ */ new Date(), options = {}) {
  const labels = { ...RECENCY_LABELS, ...options.labels };
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return { key: "older", bucket: "older", label: labels.older };
  const today = calendarDay(now, options.timeZone);
  const target = calendarDay(date, options.timeZone);
  const age = dayNumber(today) - dayNumber(target);
  if (age <= 0) return { key: "today", bucket: "today", label: labels.today };
  if (age === 1) return { key: "yesterday", bucket: "yesterday", label: labels.yesterday };
  if (age <= 7) return { key: "week", bucket: "week", label: labels.week };
  if (age <= 30) return { key: "month", bucket: "month", label: labels.month };
  const sameYear = target.year === today.year;
  const label = new Intl.DateTimeFormat(options.locale ?? "en-US", {
    month: "long",
    ...sameYear ? {} : { year: "numeric" },
    timeZone: "UTC"
  }).format(new Date(Date.UTC(target.year, target.month, 15)));
  return {
    key: `${target.year}-${String(target.month + 1).padStart(2, "0")}`,
    bucket: "older",
    label
  };
}
function groupByRecency(items, getDate, now = /* @__PURE__ */ new Date(), options = {}) {
  const timed = items.map((item) => {
    const time = toDate(getDate(item)).getTime();
    return { item, time: Number.isNaN(time) ? Number.NEGATIVE_INFINITY : time };
  });
  timed.sort((a, b) => b.time === a.time ? 0 : b.time > a.time ? 1 : -1);
  const groups = [];
  const byKey = /* @__PURE__ */ new Map();
  for (const { item } of timed) {
    const { key, bucket, label } = recencyGroupFor(getDate(item), now, options);
    let group = byKey.get(key);
    if (!group) {
      group = { key, bucket, label, items: [] };
      byKey.set(key, group);
      groups.push(group);
    }
    group.items.push(item);
  }
  return groups;
}
export {
  RECENCY_LABELS,
  groupByRecency,
  recencyGroupFor
};
