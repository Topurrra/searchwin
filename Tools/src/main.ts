import { mount } from "svelte";
import "./styles.css";
import App from "./routes/+page.svelte";

const app = mount(App, {
    target: document.getElementById("app")!,
});

export default app;