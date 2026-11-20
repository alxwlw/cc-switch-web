import * as React from "react";
import { useTranslation } from "react-i18next";
import { HoverTip } from "@/components/ui/hover-tip";

/**
 * WEB-00: when `active` (the caller passes `isWeb`, read at render) it wraps
 * an already disabled control in the `web.unavailable` hint. A disabled button gets no
 * pointer events, so the hint goes on the wrapper. Without `active` the children are returned as is.
 */
export function WebUnavailableTip({
  active,
  children,
}: {
  active: boolean;
  children: React.ReactElement;
}) {
  const { t } = useTranslation();
  if (!active) return children;
  return (
    <HoverTip content={t("web.unavailable")}>
      <span className="inline-flex [&>*]:pointer-events-none">{children}</span>
    </HoverTip>
  );
}
