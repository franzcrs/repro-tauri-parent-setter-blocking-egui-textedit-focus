import { DraftingCompass } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

function App() {

  const clickHandler = () => {
    invoke('open_tauri_widnow');
  }

  return (
    <div className="min-h-screen bg-gradient-to-b from-white from-70% to-gray-200 flex flex-col">
      <div className="my-auto p-8 text-center space-y-4">
        <div className="text-xl text-gray-900 flex items-center justify-center">
          This is my app <DraftingCompass className="ml-2 text-blue-500 flex-none"/>
        </div>
        <button className="rounded-lg px-5 py-2 text-base text-gray-900 bg-gray-300 cursor-pointer transition-colors duration-200 hover:bg-gray-200" onClick={() => clickHandler()}>
          Open Tauri Window
        </button>
        <p className="text-sm text-gray-500">
          Edit <code>src/App.tsx</code> and save to test HMR
        </p>
      </div>
    </div>
  );
}

export default App;
