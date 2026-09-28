"use client";

import Link from "next/link";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { ApiError, requestJson } from "@/lib/api/client";
import styles from "./auth-page.module.css";

type AuthMode = "login" | "signup";
type Step = "email" | "code";
type PendingAction = "request" | "resend" | "verify" | null;
type ThemeWindow = Window & {
  __setTheme?: (value: "light" | "dark") => void;
};

export function AuthPage({ mode }: { mode: AuthMode }) {
  const isSignup = mode === "signup";
  const [step, setStep] = useState<Step>("email");
  const [email, setEmail] = useState("");
  const [code, setCode] = useState("");
  const [pending, setPending] = useState<PendingAction>(null);
  const [emailError, setEmailError] = useState("");
  const [codeError, setCodeError] = useState("");
  const [status, setStatus] = useState("");
  const emailInput = useRef<HTMLInputElement>(null);
  const codeInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (step === "email") emailInput.current?.focus();
    else codeInput.current?.focus();
  }, [step]);

  async function requestCode(address: string, resend = false) {
    setPending(resend ? "resend" : "request");
    setEmailError("");
    setCodeError("");
    setStatus("");

    try {
      await requestJson("/auth/request-code", {
        method: "POST",
        body: JSON.stringify({ email: address }),
      });
      if (resend) {
        setCode("");
        setStatus("若此信箱可使用，請使用最新取得的驗證碼。");
      } else {
        setEmail(address);
        setCode("");
        setStep("code");
        setStatus("若此信箱可使用，請依此執行個體設定的方式取得驗證碼。");
      }
    } catch (cause) {
      setStatus(
        cause instanceof ApiError && cause.status === 429
          ? "操作太頻繁，請稍後再試。"
          : "目前無法取得驗證碼，請稍後再試。",
      );
    } finally {
      setPending(null);
    }
  }

  function submitEmail(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const input = emailInput.current;
    if (!input) return;

    const address = input.value.trim();
    input.value = address;
    if (!input.checkValidity()) {
      setEmailError("請輸入有效的電子郵件地址。");
      input.focus();
      return;
    }

    setEmail(address);
    void requestCode(address);
  }

  async function submitCode(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = code.replace(/\D/g, "").slice(0, 6);
    setCode(value);
    if (!/^\d{6}$/.test(value)) {
      setCodeError("請輸入 6 位數字驗證碼。");
      codeInput.current?.focus();
      return;
    }

    setPending("verify");
    setCodeError("");
    setStatus("");
    try {
      await requestJson("/auth/verify", {
        method: "POST",
        body: JSON.stringify({ email, code: value }),
      });
      window.location.assign(new URL("/", window.location.href).href);
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 401) {
        setCodeError("驗證碼錯誤或已過期，請重新確認。");
      } else {
        setStatus(
          cause instanceof ApiError && cause.status === 429
            ? "操作太頻繁，請稍後再試。"
            : "目前無法完成驗證，請稍後再試。",
        );
      }
    } finally {
      setPending(null);
    }
  }

  function changeEmail() {
    setStep("email");
    setCode("");
    setEmailError("");
    setCodeError("");
    setStatus("");
  }

  function toggleTheme() {
    const themeWindow = window as ThemeWindow;
    themeWindow.__setTheme?.(
      document.documentElement.dataset.theme === "dark" ? "light" : "dark",
    );
  }

  return (
    <div className={styles.authShell}>
      <header className={styles.authTop}>
        <Link
          aria-label="North 首頁"
          className={styles.brand}
          href={isSignup ? "/login" : "/"}
        >
          <span aria-hidden="true" className={styles.logoMark}>
            N
          </span>
          <span>North</span>
        </Link>
        <button
          aria-label="切換主題"
          className={styles.themeButton}
          title="切換主題"
          type="button"
          onClick={toggleTheme}
        >
          <svg aria-hidden="true" className={styles.themeSun} viewBox="0 0 24 24">
            <circle cx="12" cy="12" r="4" />
            <path d="M12 2v2m0 16v2M4.93 4.93l1.42 1.42m11.3 11.3 1.42 1.42M2 12h2m16 0h2M4.93 19.07l1.42-1.42m11.3-11.3 1.42-1.42" />
          </svg>
          <svg aria-hidden="true" className={styles.themeMoon} viewBox="0 0 24 24">
            <path d="M20.4 15.2A8.5 8.5 0 0 1 8.8 3.6 8.5 8.5 0 1 0 20.4 15.2Z" />
          </svg>
        </button>
      </header>

      <main className={styles.authMain}>
        <div className={styles.authWrap}>
          <section
            aria-labelledby="email-title"
            className={styles.authPanel}
            data-testid="auth-panel"
            hidden={step !== "email"}
          >
            <h1 className={styles.authHeading} id="email-title">
              {isSignup ? "建立 North 帳戶" : "登入 North"}
            </h1>
            <p className={styles.authLead}>
              {isSignup
                ? "使用電子郵件地址建立帳戶，不需要設定密碼。"
                : "使用電子郵件驗證碼登入；首次驗證時會建立帳戶。"}
            </p>
            <form className={styles.authForm} noValidate onSubmit={submitEmail}>
              <div className={styles.field}>
                <label htmlFor="auth-email">電子郵件地址</label>
                <input
                  ref={emailInput}
                  autoComplete="email"
                  className={styles.input}
                  id="auth-email"
                  inputMode="email"
                  name="email"
                  placeholder="例如：name@example.com"
                  required
                  type="email"
                  aria-describedby="email-help email-error"
                  aria-invalid={emailError ? true : undefined}
                  value={email}
                  onChange={(event) => {
                    setEmail(event.target.value);
                    setEmailError("");
                    setStatus("");
                  }}
                />
                <span className={styles.fieldHelp} id="email-help">
                  驗證碼依此 North 執行個體設定的交付方式提供。
                </span>
                <span className={styles.fieldError} id="email-error" role="alert" hidden={!emailError}>
                  {emailError}
                </span>
              </div>
              <button
                aria-busy={pending === "request"}
                className={styles.primaryButton}
                disabled={pending !== null}
                type="submit"
              >
                {pending === "request" ? "正在取得…" : "取得驗證碼"}
                {pending === "request" && <span aria-hidden="true" className={styles.spinner} />}
              </button>
              <p aria-live="polite" className={styles.statusMessage} role="status" hidden={!status}>
                {status}
              </p>
            </form>
            <p className={styles.authNote}>
              {isSignup
                ? "此工作區第一位建立帳戶的成員會成為 Owner；後續帳戶預設為 Requester。"
                : "不需要密碼。每次登入都會使用新的驗證碼。"}
            </p>
          </section>

          <section
            aria-labelledby="code-title"
            className={styles.authPanel}
            hidden={step !== "code"}
          >
            <div className={styles.verifyHead}>
              <button
                aria-label="返回並更改電子郵件"
                className={styles.textButton}
                disabled={pending !== null}
                type="button"
                onClick={changeEmail}
              >
                <svg aria-hidden="true" viewBox="0 0 24 24">
                  <path d="m15 18-6-6 6-6M9 12h12" />
                </svg>
                <span>更改電子郵件</span>
              </button>
              <div>
                <h1 className={styles.authHeading} id="code-title">
                  {isSignup ? "驗證電子郵件" : "輸入驗證碼"}
                </h1>
                <p className={styles.authLead}>
                  請輸入此地址最新的 6 位數驗證碼。
                  <span className={styles.emailPreview}>{email}</span>
                </p>
              </div>
            </div>
            <form className={styles.authForm} noValidate onSubmit={submitCode}>
              <div className={styles.field}>
                <label htmlFor="auth-code">6 位數驗證碼</label>
                <input
                  ref={codeInput}
                  autoComplete="one-time-code"
                  className={`${styles.input} ${styles.codeInput}`}
                  id="auth-code"
                  inputMode="numeric"
                  maxLength={6}
                  name="code"
                  pattern="[0-9]{6}"
                  required
                  type="text"
                  aria-describedby="code-help code-error"
                  aria-invalid={codeError ? true : undefined}
                  value={code}
                  onChange={(event) => {
                    setCode(event.target.value.replace(/\D/g, "").slice(0, 6));
                    setCodeError("");
                  }}
                />
                <span className={styles.fieldHelp} id="code-help">
                  每組驗證碼僅能使用一次；重新取得後請使用最新一組。
                </span>
                <span className={styles.fieldError} id="code-error" role="alert" hidden={!codeError}>
                  {codeError}
                </span>
              </div>
              <div className={styles.verifyActions}>
                <button
                  aria-busy={pending === "verify"}
                  className={styles.primaryButton}
                  disabled={pending !== null}
                  type="submit"
                >
                  {pending === "verify"
                    ? isSignup
                      ? "正在建立帳戶…"
                      : "正在登入…"
                    : isSignup
                      ? "完成註冊"
                      : "登入"}
                  {pending === "verify" && <span aria-hidden="true" className={styles.spinner} />}
                </button>
                <button
                  aria-busy={pending === "resend"}
                  className={styles.textButton}
                  disabled={pending !== null}
                  type="button"
                  onClick={() => void requestCode(email, true)}
                >
                  重新取得驗證碼
                  {pending === "resend" && <span aria-hidden="true" className={styles.spinner} />}
                </button>
              </div>
              <p aria-live="polite" className={styles.statusMessage} role="status" hidden={!status}>
                {status}
              </p>
            </form>
          </section>

          <p className={styles.authCaption}>
            {isSignup ? "已經有帳戶？ " : "還沒有帳戶？ "}
            <Link className={styles.authLink} href={isSignup ? "/login" : "/signup"}>
              {isSignup ? "登入" : "建立帳戶"}
            </Link>
          </p>
        </div>
      </main>
    </div>
  );
}
