import React, { useEffect, useState } from "react";
import { ErrorDetails, RqbitAPI } from "../../api-types";
import { Button } from "../buttons/Button";
import { FormInput } from "../forms/FormInput";
import { Spinner } from "../Spinner";

type CategoryAPI = Required<
  Pick<RqbitAPI, "listCategories" | "createCategory" | "removeCategories">
>;

const errorText = (error: unknown): string => {
  const details = error as ErrorDetails;
  return (
    (typeof details.text === "string" ? details.text : undefined) ||
    details.statusText ||
    "Category operation failed"
  );
};

export const CategoriesTab: React.FC<{ api: CategoryAPI }> = ({ api }) => {
  const [categories, setCategories] = useState<string[]>([]);
  const [newCategory, setNewCategory] = useState("");
  const [loading, setLoading] = useState(true);
  const [busyCategory, setBusyCategory] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setCategories(await api.listCategories());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const addCategory = async () => {
    const name = newCategory.trim();
    if (!name) return;

    setBusyCategory(name);
    setError(null);
    try {
      await api.createCategory(name);
      setNewCategory("");
      await refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusyCategory(null);
    }
  };

  const removeCategory = async (name: string) => {
    if (
      !window.confirm(
        `Remove category "${name}"? Assigned torrents will become uncategorized; their files will not be deleted.`,
      )
    ) {
      return;
    }

    setBusyCategory(name);
    setError(null);
    try {
      await api.removeCategories([name]);
      await refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusyCategory(null);
    }
  };

  if (loading && categories.length === 0) {
    return (
      <div className="flex justify-center p-4">
        <Spinner />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 py-2">
      <div className="flex items-end gap-2">
        <div className="grow">
          <FormInput
            label="New category"
            name="new_category"
            placeholder="For example: tv"
            value={newCategory}
            disabled={busyCategory !== null}
            onChange={(event) => setNewCategory(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void addCategory();
              }
            }}
          />
        </div>
        <div className="mb-2">
          <Button
            variant="primary"
            onClick={() => void addCategory()}
            disabled={!newCategory.trim() || busyCategory !== null}
          >
            Add
          </Button>
        </div>
      </div>

      {error && (
        <div className="bg-error/10 text-error rounded border border-error p-2 text-sm">
          {error}
        </div>
      )}

      <div className="divide-y divide-divider rounded border border-divider">
        {categories.length === 0 ? (
          <div className="p-3 text-sm text-secondary">
            No categories configured.
          </div>
        ) : (
          categories.map((category) => (
            <div
              key={category}
              className="flex items-center justify-between gap-3 p-2"
            >
              <span className="break-all">{category}</span>
              <Button
                variant="danger"
                size="sm"
                onClick={() => void removeCategory(category)}
                disabled={busyCategory !== null}
              >
                Remove
              </Button>
            </div>
          ))
        )}
      </div>

      <p className="text-sm text-secondary">
        Removing a category only unassigns its torrents. Downloaded data is not
        removed.
      </p>
    </div>
  );
};
