import { Navigate, Route, Routes } from "react-router-dom";
import { Layout } from "./Layout";
import { SessionsPage } from "./pages/SessionsPage";
import { RecordingsPage } from "./pages/RecordingsPage";
import { BrowsersPage } from "./pages/BrowsersPage";
import { UsersPage } from "./components/UsersPage";
import { QuotasPage } from "./components/QuotasPage";

export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Navigate to="sessions" replace />} />
        <Route path="sessions" element={<SessionsPage />} />
        <Route path="recordings" element={<RecordingsPage />} />
        <Route path="browsers" element={<BrowsersPage />} />
        <Route path="users" element={<UsersPage />} />
        <Route path="quotas" element={<QuotasPage />} />
        <Route path="*" element={<Navigate to="sessions" replace />} />
      </Route>
    </Routes>
  );
}
