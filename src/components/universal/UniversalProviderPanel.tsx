import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Layers } from "lucide-react";
import { toast } from "@/lib/toast";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { UniversalProviderCard } from "./UniversalProviderCard";
import { UniversalProviderFormModal } from "./UniversalProviderFormModal";
import { universalProvidersApi } from "@/lib/api";
import type { UniversalProvider, UniversalProvidersMap } from "@/types";
import { deepClone } from "@/utils/deepClone";

const errorText = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);

export function UniversalProviderPanel() {
  const { t } = useTranslation();

  // 状态
  const [providers, setProviders] = useState<UniversalProvidersMap>({});
  const [loading, setLoading] = useState(true);
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingProvider, setEditingProvider] =
    useState<UniversalProvider | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<{
    open: boolean;
    id: string;
    name: string;
  }>({ open: false, id: "", name: "" });
  const [syncConfirm, setSyncConfirm] = useState<{
    open: boolean;
    id: string;
    name: string;
  }>({ open: false, id: "", name: "" });

  // 加载数据
  const loadProviders = useCallback(async () => {
    try {
      setLoading(true);
      const data = await universalProvidersApi.getAll();
      setProviders(data);
    } catch (error) {
      console.error("Failed to load universal providers:", error);
      toast.error(
        t("universalProvider.loadError", {
          defaultValue: "加载统一供应商失败",
        }),
      );
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  // upsert 与 sync 分开处理（同 AddProviderDialog）：upsert 失败 = 什么都没存；
  // upsert 成功而 sync 失败 = 统一供应商已入库，只是部分应用的配置文件没写成，
  // 此时给 warning（带服务端原文）并照常刷新列表。返回 false 表示 upsert 就失败了。
  const upsertThenSync = useCallback(
    async (
      provider: UniversalProvider,
      opts: { sync: boolean; savedKey: string; savedDefault: string },
      failure: { key: string; defaultValue: string },
    ): Promise<boolean> => {
      try {
        await universalProvidersApi.upsert(provider);
      } catch (error) {
        console.error("Failed to save universal provider:", error);
        toast.error(t(failure.key, { defaultValue: failure.defaultValue }));
        return false;
      }
      if (opts.sync) {
        try {
          await universalProvidersApi.sync(provider.id);
        } catch (error) {
          console.error("Universal provider saved but sync failed:", error);
          toast.warning(
            t("universalProvider.addedButSyncFailed", {
              defaultValue: "统一供应商已添加，但同步失败",
            }),
            { description: errorText(error) },
          );
          loadProviders();
          return true;
        }
      }
      toast.success(t(opts.savedKey, { defaultValue: opts.savedDefault }));
      loadProviders();
      return true;
    },
    [loadProviders, t],
  );

  // 添加/编辑供应商
  const handleSave = useCallback(
    async (provider: UniversalProvider) => {
      const ok = await upsertThenSync(
        provider,
        editingProvider
          ? {
              sync: false,
              savedKey: "universalProvider.updated",
              savedDefault: "统一供应商已更新",
            }
          : {
              // 新建模式下自动同步到各应用
              sync: true,
              savedKey: "universalProvider.addedAndSynced",
              savedDefault: "统一供应商已添加并同步",
            },
        {
          key: "universalProvider.saveError",
          defaultValue: "保存统一供应商失败",
        },
      );
      if (ok) setEditingProvider(null);
    },
    [editingProvider, upsertThenSync],
  );

  // 保存并同步供应商
  const handleSaveAndSync = useCallback(
    async (provider: UniversalProvider) => {
      const ok = await upsertThenSync(
        provider,
        {
          sync: true,
          savedKey: "universalProvider.savedAndSynced",
          savedDefault: "已保存并同步到所有应用",
        },
        {
          key: "universalProvider.saveAndSyncError",
          defaultValue: "保存并同步失败",
        },
      );
      if (ok) setEditingProvider(null);
    },
    [upsertThenSync],
  );

  // 删除供应商
  const handleDelete = useCallback(async () => {
    if (!deleteConfirm.id) return;

    try {
      await universalProvidersApi.delete(deleteConfirm.id);
      toast.success(
        t("universalProvider.deleted", { defaultValue: "统一供应商已删除" }),
      );
      loadProviders();
    } catch (error) {
      console.error("Failed to delete universal provider:", error);
      toast.error(
        t("universalProvider.deleteError", {
          defaultValue: "删除统一供应商失败",
        }),
      );
    } finally {
      setDeleteConfirm({ open: false, id: "", name: "" });
    }
  }, [deleteConfirm.id, loadProviders, t]);

  // 同步供应商
  const handleSync = useCallback(async () => {
    if (!syncConfirm.id) return;

    try {
      await universalProvidersApi.sync(syncConfirm.id);
      toast.success(
        t("universalProvider.synced", { defaultValue: "已同步到所有应用" }),
      );
    } catch (error) {
      console.error("Failed to sync universal provider:", error);
      // sync 只写各应用的子供应商，不改统一供应商记录本身，所以不刷新本列表
      toast.error(
        t("universalProvider.syncError", {
          defaultValue: "同步统一供应商失败",
        }),
        { description: errorText(error) },
      );
    } finally {
      setSyncConfirm({ open: false, id: "", name: "" });
    }
  }, [syncConfirm.id, t]);

  // 打开同步确认
  const handleSyncClick = useCallback(
    (id: string) => {
      const provider = providers[id];
      setSyncConfirm({
        open: true,
        id,
        name: provider?.name || id,
      });
    },
    [providers],
  );

  // 复制供应商
  const handleDuplicate = useCallback(
    async (provider: UniversalProvider) => {
      const duplicated: UniversalProvider = {
        ...deepClone(provider),
        id: crypto.randomUUID(),
        name: `${provider.name} copy`,
        createdAt: Date.now(),
      };
      await upsertThenSync(
        duplicated,
        {
          sync: true,
          savedKey: "universalProvider.duplicatedAndSynced",
          savedDefault: "统一供应商已复制并同步",
        },
        {
          key: "universalProvider.duplicateError",
          defaultValue: "复制统一供应商失败",
        },
      );
    },
    [upsertThenSync],
  );

  // 打开编辑
  const handleEdit = useCallback((provider: UniversalProvider) => {
    setEditingProvider(provider);
    setIsFormOpen(true);
  }, []);

  // 打开删除确认
  const handleDeleteClick = useCallback(
    (id: string) => {
      const provider = providers[id];
      setDeleteConfirm({
        open: true,
        id,
        name: provider?.name || id,
      });
    },
    [providers],
  );

  const providerList = Object.values(providers);

  return (
    <div className="space-y-4">
      {/* 头部 */}
      <div className="flex items-center gap-2">
        <Layers className="h-5 w-5 text-primary" />
        <h2 className="text-lg font-semibold">
          {t("universalProvider.title", { defaultValue: "统一供应商" })}
        </h2>
        <span className="rounded-full bg-subtle px-2 py-0.5 text-xs text-fg-2">
          {providerList.length}
        </span>
      </div>

      {/* 描述 */}
      <p className="text-sm text-fg-2">
        {t("universalProvider.description", {
          defaultValue:
            "统一供应商可以同时管理 Claude、Codex 和 Gemini 的配置。修改后会自动同步到所有启用的应用。",
        })}
      </p>

      {/* 供应商列表 */}
      {loading ? (
        <div className="flex items-center justify-center py-12">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        </div>
      ) : providerList.length === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-xl border border-dashed py-12 text-center">
          <Layers className="mb-3 h-10 w-10 text-fg-3" />
          <p className="text-sm text-fg-2">
            {t("universalProvider.empty", {
              defaultValue: "还没有统一供应商",
            })}
          </p>
          <p className="mt-1 text-xs text-fg-3">
            {t("universalProvider.emptyHint", {
              defaultValue: "点击下方「添加统一供应商」按钮创建一个",
            })}
          </p>
        </div>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {providerList.map((provider) => (
            <UniversalProviderCard
              key={provider.id}
              provider={provider}
              onEdit={handleEdit}
              onDelete={handleDeleteClick}
              onSync={handleSyncClick}
              onDuplicate={handleDuplicate}
            />
          ))}
        </div>
      )}

      {/* 表单模态框 */}
      <UniversalProviderFormModal
        isOpen={isFormOpen}
        onClose={() => {
          setIsFormOpen(false);
          setEditingProvider(null);
        }}
        onSave={handleSave}
        onSaveAndSync={handleSaveAndSync}
        editingProvider={editingProvider}
      />

      {/* 删除确认对话框 */}
      <ConfirmDialog
        isOpen={deleteConfirm.open}
        title={t("universalProvider.deleteConfirmTitle", {
          defaultValue: "删除统一供应商",
        })}
        message={t("universalProvider.deleteConfirmDescription", {
          defaultValue: `确定要删除 "${deleteConfirm.name}" 吗？这将同时删除它在各应用中生成的供应商配置。`,
          name: deleteConfirm.name,
        })}
        confirmText={t("common.delete", { defaultValue: "删除" })}
        onConfirm={handleDelete}
        onCancel={() => setDeleteConfirm({ open: false, id: "", name: "" })}
      />

      {/* 同步确认对话框 */}
      <ConfirmDialog
        isOpen={syncConfirm.open}
        title={t("universalProvider.syncConfirmTitle", {
          defaultValue: "同步统一供应商",
        })}
        message={t("universalProvider.syncConfirmDescription", {
          defaultValue: `同步 "${syncConfirm.name}" 将会覆盖 Claude、Codex 和 Gemini 中关联的供应商配置。确定要继续吗？`,
          name: syncConfirm.name,
        })}
        confirmText={t("universalProvider.syncConfirm", {
          defaultValue: "同步",
        })}
        onConfirm={handleSync}
        onCancel={() => setSyncConfirm({ open: false, id: "", name: "" })}
      />
    </div>
  );
}
