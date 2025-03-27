/* eslint-disable @typescript-eslint/no-explicit-any */
"use client";

import React, { useState } from "react";

export default function SpeechPage() {
  const [text, setText] = useState("");

  function handleSubmit(e: any) {
    e.preventDefault();

    // Fire-and-forget: just call fetch with no awaits or .then handlers
    fetch("/api/speech", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ input: text }),
    });

    // (Optional) You can clear the text or otherwise give user feedback immediately:
    setText("");
  }

  return (
    <div className="p-4 max-w-xl mx-auto">
      <h1 className="text-xl font-bold mb-4">
        Text-to-Speech Demo (Fire & Forget)
      </h1>
      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <label className="block mb-1 font-medium">
            Text to Speak (no response):
          </label>
          <textarea
            rows={5}
            value={text}
            onChange={(e) => setText(e.target.value)}
            className="border w-full p-2 text-black"
          />
        </div>
        <button
          type="submit"
          className="px-4 py-2 bg-blue-600 text-white rounded"
        >
          Send Request
        </button>
      </form>
    </div>
  );
}
