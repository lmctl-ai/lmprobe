const navLinks = Array.from(document.querySelectorAll(".toc a"));
const sections = navLinks
  .map((link) => document.querySelector(link.getAttribute("href")))
  .filter(Boolean);

const activateLink = () => {
  let active = sections[0];
  for (const section of sections) {
    if (section.getBoundingClientRect().top <= 120) {
      active = section;
    }
  }
  navLinks.forEach((link) => {
    link.classList.toggle("active", link.getAttribute("href") === `#${active.id}`);
  });
};

window.addEventListener("scroll", activateLink, { passive: true });
activateLink();

document.querySelectorAll("pre").forEach((block) => {
  const codeElement = block.querySelector("code");
  const button = document.createElement("button");
  button.className = "copy-button";
  button.type = "button";
  button.setAttribute("aria-label", "Copy code block");
  button.textContent = "Copy";
  button.addEventListener("click", async () => {
    const code = codeElement ? codeElement.innerText : block.innerText;
    try {
      await navigator.clipboard.writeText(code);
      button.textContent = "Copied";
      setTimeout(() => {
        button.textContent = "Copy";
      }, 1400);
    } catch {
      button.textContent = "Select";
      setTimeout(() => {
        button.textContent = "Copy";
      }, 1400);
    }
  });
  block.appendChild(button);
});

const search = document.querySelector("#recipeSearch");
const recipes = Array.from(document.querySelectorAll(".recipe-card"));

if (search) {
  search.addEventListener("input", () => {
    const q = search.value.trim().toLowerCase();
    recipes.forEach((card) => {
      const text = `${card.textContent} ${card.dataset.tags || ""}`.toLowerCase();
      card.classList.toggle("hidden", q.length > 0 && !text.includes(q));
    });
  });
}
