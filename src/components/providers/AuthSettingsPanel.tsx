import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import type { AuthCenterTarget } from "@/lib/api";
import { isViewAvailable } from "@/lib/navigation";

interface AuthSettingsPanelProps {
  target: AuthCenterTarget | null;
  onClose: () => void;
}

export function AuthSettingsPanel({ target, onClose }: AuthSettingsPanelProps) {
  const { t } = useTranslation();
  const isOpen = target !== null;

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("nav.auth")}
      onClose={onClose}
      motionPreset="slide-from-right"
    >
      {/* WEB-00: ссылки «управлять аккаунтами» из форм — в web вместо бросающей панели notice — снимет WEB-02 */}
      {target && !isViewAvailable("auth") ? (
        <p role="status" className="text-body text-fg-2">
          {t("web.unavailable")}
        </p>
      ) : target ? (
        <AuthCenterPanel authScrollTarget={target} />
      ) : null}
    </FullScreenPanel>
  );
}
