import { useState } from "react";
import { Button } from "../buttons/Button";
import { Spinner } from "../Spinner";
import { Modal } from "./Modal";
import { ModalBody } from "./ModalBody";
import { ModalFooter } from "./ModalFooter";

export const LogoutModal: React.FC<{
  show: boolean;
  onHide: () => void;
  onLogout: () => Promise<void>;
}> = ({ show, onHide, onLogout }) => {
  const [loggingOut, setLoggingOut] = useState(false);
  const [failed, setFailed] = useState(false);

  if (!show) return null;

  const close = () => {
    if (loggingOut) return;
    setFailed(false);
    onHide();
  };

  const logout = async () => {
    setLoggingOut(true);
    setFailed(false);
    try {
      await onLogout();
    } catch (error) {
      console.error("Logout failed", error);
      setFailed(true);
      setLoggingOut(false);
    }
  };

  return (
    <Modal
      isOpen={show}
      onClose={loggingOut ? undefined : close}
      title="Log out"
    >
      <ModalBody>
        <p className="text-secondary">
          Are you sure you want to log out of rqbit?
        </p>
        {failed && (
          <p
            className="bg-error-bg/10 text-error rounded mt-3 px-3 py-2"
            role="alert"
          >
            Unable to log out. Please try again.
          </p>
        )}
      </ModalBody>
      <ModalFooter>
        {loggingOut && <Spinner />}
        <Button variant="cancel" onClick={close} disabled={loggingOut}>
          Cancel
        </Button>
        <Button variant="danger" onClick={logout} disabled={loggingOut}>
          Log out
        </Button>
      </ModalFooter>
    </Modal>
  );
};
