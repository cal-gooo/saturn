import "./site.css";
import DocsPage from "./DocsPage.svelte";
import { mount } from "svelte";

mount(DocsPage, {
  target: document.getElementById("app")
});
