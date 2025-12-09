# Reddit Launch Post Draft
**Title:** I built a filesystem in Rust that organizes itself and has "Time Travel" built-in.

**Body:**

Hey everyone,

I've spent the last few months fighting with FUSE (Filesystem in Userspace) to build something I've always wanted: a filesystem that actually does work for me.

It's called **Eidetic**.

**What it does:**
It mirrors any folder on your computer but adds "superpowers" to it:

1.  **Time Travel**: Every time you save a file, Eidetic creates an instant snapshot. You can browse history just by looking in a magic `.history` folder. No git commands needed.
2.  **The Vault**: Drag anything into the `/vault` folder, and it's encrypted on disk instantly. It looks like garbage data to anyone else, but readable to you through the mountpoint.
3.  **Auto-Organization**: If I save a PDF invoice, it detects the content and moves it to my `/Finance` folder automatically.
4.  **Context for AI**: I added a feature for us devs. if you run `cat .context`, it bundles your whole project into a perfect prompt for ChatGPT/Claude.

**Tech Stack:**
- **Rust** (for safety and speed)
- **FUSE** (fuser crate)
- **SQLite** (for metadata handling)

It's still experimental, but I'm using it daily to manage my chaotic downloads folder.

**Repo / Download:** [Link to GitHub/Website]

Would love to hear what features you'd want in a "smart" filesystem!

Cheers,
[Your Name]
